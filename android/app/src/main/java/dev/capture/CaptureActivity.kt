package dev.capture

import android.Manifest
import android.content.pm.PackageManager
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.capture_ffi.Captured

/**
 * Type something and commit it.
 *
 * The phone's equivalent of the desktop overlay, and it makes the same
 * promises: the field is focused when it appears, one gesture commits, and the
 * window is gone immediately afterwards. §2.1's argument is that capture has
 * to cost almost nothing at the moment a thought arrives, and anything that
 * asks for a second decision — a category, a confirmation — has already spent
 * more than the thought is worth.
 *
 * It closes *after* the commit returns, not before. Dismissing first would
 * make the interface faster and would mean reporting success for something
 * that had not happened yet.
 */
class CaptureActivity : ComponentActivity() {

    private lateinit var recorder: Recorder

    /**
     * Asking to use the microphone, at the moment somebody reaches for it.
     *
     * Not at launch. A permission prompt before anything has explained why is
     * the one most often refused permanently, and capture works without this
     * — typing is the primary gesture and voice is the one for when your
     * hands are busy.
     */
    private val askMicrophone = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        if (!granted) {
            Toast.short(this, "Without the microphone there is nothing to record.")
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        recorder = Recorder(this)

        // Shown over the lock screen, and the device is not unlocked to do it.
        // The tile is reachable from there; an activity that then demanded a
        // PIN would put the whole gesture behind the thing it was avoiding.
        setShowWhenLocked(true)

        setContent {
            MaterialTheme {
                Surface {
                    CaptureField(
                        onCommit = { text -> commit(text) },
                        onDismiss = { finish() },
                        onHold = { held() },
                        onRelease = { spoken() },
                    )
                }
            }
        }
    }

    /**
     * A finger went down on the microphone.
     *
     * Recording starts here rather than on release, because the alternative is
     * a device that starts listening after the sentence has begun. If the
     * permission is missing this asks for it and records nothing — the next
     * hold works, and the first one is honest about having done nothing.
     */
    private fun held() {
        val allowed = ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) ==
            PackageManager.PERMISSION_GRANTED
        if (!allowed) {
            askMicrophone.launch(Manifest.permission.RECORD_AUDIO)
            return
        }
        if (!recorder.start()) {
            Toast.short(this, "The microphone could not be opened.")
        }
    }

    /** And came up again. */
    private fun spoken() {
        val said = recorder.stop()
        if (said == null) {
            // A tap rather than a hold, or a recorder that produced nothing.
            // Said plainly: a gesture that recorded silence and committed it
            // leaves a note somebody has to open to discover is empty.
            Toast.short(this, "Hold to talk — that was too short to record.")
            return
        }

        lifecycleScope.launch {
            val outcome = withContext(Dispatchers.IO) {
                runCatching {
                    Store.of(this@CaptureActivity)
                        .captureVoice(said.bytes, Recorder.MEDIA_TYPE, said.durationMs)
                }
            }
            outcome.onFailure {
                // The bytes are gone with the scratch file, so this is the
                // only chance to say so. A recording reported as saved and
                // absent is worse than one that failed loudly.
                Toast.short(this@CaptureActivity, it.message ?: "The recording was not saved.")
            }
            finish()
        }
    }

    private fun commit(text: String) {
        if (text.isBlank()) {
            finish()
            return
        }
        lifecycleScope.launch {
            // Off the main thread, and awaited. The store commits before it
            // returns; this only keeps the commit off the frame deadline.
            val result = withContext(Dispatchers.IO) { Store.of(this@CaptureActivity).capture(text) }

            // Said out loud when it went to the spool. Both outcomes are safe,
            // but they differ in whether it can be found yet, and a surface
            // that reported them identically would be claiming something it
            // does not know.
            if (result is Captured.Spooled) {
                Toast.short(this@CaptureActivity, "Saved. It will be filed when the store is writable.")
            }
            finish()
        }
    }
}

@Composable
private fun CaptureField(
    onCommit: (String) -> Unit,
    onDismiss: () -> Unit,
    onHold: () -> Unit,
    onRelease: () -> Unit,
) {
    var text by remember { mutableStateOf("") }
    val focus = remember { FocusRequester() }

    Column(
        modifier = Modifier.fillMaxWidth().padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        TextField(
            value = text,
            onValueChange = { text = it },
            modifier = Modifier.fillMaxWidth().focusRequester(focus),
            placeholder = { Text("") },
            // Enter commits, exactly as it does on the desktop overlay.
            //
            // `singleLine` is what makes that true, and it is not a styling
            // choice. A multi-line field swallows Enter as a newline and the
            // IME action never fires — so the first version of this looked
            // right, accepted the text, and did nothing at all when the key
            // was pressed. §0.2 argues that the capture gesture must not
            // differ between devices; a commit key that works on one and
            // silently inserts whitespace on the other is exactly that
            // difference, and the kind nobody reports because it reads as
            // their own mistake.
            singleLine = true,
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = { onCommit(text) }),
        )
        Button(onClick = { onCommit(text) }, modifier = Modifier.fillMaxWidth()) {
            Text("Capture")
        }

        // Hold, rather than tap-to-start and tap-to-stop.
        //
        // Two taps means a state a person can leave the device in by walking
        // away — a phone recording in a pocket until the battery goes. A hold
        // cannot outlive the hand doing it, which is the same argument §2.1
        // makes for the capture gesture costing nothing: the failure mode of
        // forgetting is designed out rather than warned about.
        // Not a `Button`.
        //
        // A Material button has its own `clickable` inside it, which consumes
        // the pointer before a `pointerInput` on the same chain ever sees it —
        // so the first version of this rendered correctly, highlighted when
        // pressed, and never started recording. Nothing failed; the gesture
        // simply went somewhere else. A surface with only this gesture on it
        // has nothing to compete with.
        var talking by remember { mutableStateOf(false) }
        Surface(
            color = if (talking) {
                MaterialTheme.colorScheme.errorContainer
            } else {
                MaterialTheme.colorScheme.secondaryContainer
            },
            shape = MaterialTheme.shapes.small,
            modifier = Modifier.fillMaxWidth().pointerInput(Unit) {
                detectTapGestures(
                    onPress = {
                        talking = true
                        onHold()
                        // Suspends until the finger lifts or the gesture is
                        // cancelled, and both mean the same thing here: stop.
                        tryAwaitRelease()
                        talking = false
                        onRelease()
                    },
                )
            },
        ) {
            Text(
                if (talking) "Listening — let go to keep it" else "Hold to talk",
                modifier = Modifier.padding(16.dp),
                textAlign = TextAlign.Center,
            )
        }
    }

    androidx.compose.runtime.LaunchedEffect(Unit) { focus.requestFocus() }
    androidx.activity.compose.BackHandler { onDismiss() }
}
