package dev.capture

import android.os.Bundle
import androidx.activity.ComponentActivity
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
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
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

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

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
                    )
                }
            }
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
private fun CaptureField(onCommit: (String) -> Unit, onDismiss: () -> Unit) {
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
    }

    androidx.compose.runtime.LaunchedEffect(Unit) { focus.requestFocus() }
    androidx.activity.compose.BackHandler { onDismiss() }
}
