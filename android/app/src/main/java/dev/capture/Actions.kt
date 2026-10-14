package dev.capture

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import uniffi.capture_ffi.NoteKind
import uniffi.capture_ffi.NoteSummary
import uniffi.capture_ffi.Pending

/**
 * Everything that can be done to a note, from the note.
 *
 * §8.4 makes the reading surface the first-party one and §8.10 scoped it to
 * every mutation, for a reason that bites harder on a phone than anywhere
 * else: §2.6 exists so that suppressing something is *cheaper* than destroying
 * it, and that gradient inverts the moment muting needs a laptop and
 * destroying does not.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun Actions(note: NoteSummary, pending: Pending?, model: Model) {
    var asking by remember { mutableStateOf<Ask?>(null) }
    var destroying by remember { mutableStateOf(false) }

    pending?.let {
        // When, not how long is left. A countdown rendered once is wrong
        // within a minute, and wrong in the direction of looking safe.
        Text(
            "Will be destroyed at ${it.at} on ${it.day}.",
            Modifier.padding(start = 16.dp, end = 16.dp, top = 16.dp),
            style = MaterialTheme.typography.labelMedium,
        )
    }

    FlowRow(
        Modifier.fillMaxWidth().padding(16.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        // Full screen, not a dialog: a one-line box could not hold a page.
        TextButton({ model.go(Screen.Editor(note.slug)) }) { Text("Edit") }
        // A capture grown into a page stops being read for dates; the note
        // page offers the way back while it is one.
        if (note.kind != "page") {
            TextButton({ model.makePage(note.slug) }) { Text("Make this a page") }
        }
        TextButton({ asking = Ask("Tag", "A tag", "") { model.tag(note.slug, it) } }) { Text("Tag") }
        TextButton({
            asking = Ask("Rename", "A chosen name; empty clears it", note.alias ?: "") {
                model.setAlias(note.slug, it.ifBlank { null })
            }
        }) { Text("Rename") }
        TextButton({
            // The one filing gesture, and it says what it costs: a page put in
            // the wrong branch is still in the timeline, in search and behind
            // every link (§3.7).
            asking = Ask("Move", "Under which page; empty puts it back at the top", "") {
                model.setParent(note.slug, it.ifBlank { null })
            }
        }) { Text("Move") }
        TextButton({ model.pin(note.slug, !note.pinned) }) {
            Text(if (note.pinned) "Unpin" else "Pin")
        }
        // Saying what something is, which the archive would otherwise only
        // guess at — and, below the floor, deliberately not guess at.
        //
        // Here and not on a timeline row. A correction is pinned, so it beats
        // every later reparse, and a control on a scrolling surface is a
        // mis-tap away from writing one of those while thumbing past.
        if (note.kind != "task") {
            TextButton({ model.asTask(note.slug, null) }) { Text("It's a task") }
        }
        if (note.kind == "task") {
            TextButton({
                asking = Ask(
                    "Due",
                    "A day, as 2026-09-20. Empty clears it.",
                    "",
                ) { model.asTask(note.slug, it.ifBlank { null }) }
            }) { Text("Due") }
        }
        // A thing wrongly promoted is as ordinary as a thing wrongly left
        // alone, so the way back is the same gesture rather than a harder one.
        if (note.kind != "note") {
            TextButton({ model.setKind(note.slug, NoteKind.NOTE) }) { Text("Just a note") }
        }
        // In force, not merely recorded: a mute whose end has passed is still
        // a field on the note, and this hid "Mute" behind it for good — while
        // nothing anywhere offered "Unmute", so a phone could suppress a note
        // and never bring it back.
        if (note.muted) {
            TextButton({ model.unmute(note.slug) }) { Text("Unmute") }
        } else {
            TextButton({ model.mute(note.slug) }) { Text("Mute") }
        }
        if (note.redactionQueued) {
            // The window is the point. While it is open the content is
            // withheld and still there, and this is the only moment it can be
            // called back.
            TextButton({ model.cancelRedaction(note.slug) }) { Text("Keep it after all") }
        } else if (!note.redacted) {
            TextButton({ destroying = true }) { Text("Destroy") }
        }
    }

    asking?.let { ask ->
        Prompt(ask, onDone = { asking = null })
    }
    if (destroying) {
        ConfirmDestroy(note, onDismiss = { destroying = false }, onGo = {
            destroying = false
            model.redact(note.slug)
        })
    }
}

/** A question with a text answer. */
class Ask(
    val title: String,
    val hint: String,
    val initial: String,
    val then: (String) -> Unit,
)

@Composable
private fun Prompt(ask: Ask, onDone: () -> Unit) {
    var text by remember { mutableStateOf(ask.initial) }
    AlertDialog(
        onDismissRequest = onDone,
        title = { Text(ask.title) },
        text = {
            Column {
                Text(ask.hint, style = MaterialTheme.typography.labelSmall)
                TextField(text, { text = it }, Modifier.fillMaxWidth())
            }
        },
        confirmButton = { Button({ ask.then(text); onDone() }) { Text(ask.title) } },
        dismissButton = { TextButton(onDone) { Text("Cancel") } },
    )
}

/**
 * The only destructive confirmation, and it asks you to write something.
 *
 * A count is dismissible in a way a typed answer is not, and dismissing
 * dialogs is a reflex this one has to survive. It names what will go rather
 * than asking whether you are sure.
 *
 * # Why the last four characters and not the whole name
 *
 * It asked for the entire slug — `groceries-milk-egss-bread-8683fst3j8` — and
 * on a phone that is not a speed bump, it is a wall. A guard nobody can clear
 * is not a stronger guard: it is one that gets worked around, and the
 * available workaround here is a copy button, which turns the whole
 * confirmation into copy, paste, destroy. Three taps and no moment that
 * requires reading anything.
 *
 * Four characters of the suffix keep what the guard was actually for. They
 * cannot be produced by reflex, they are different for every note — so muscle
 * memory cannot destroy the wrong one — and they are short enough to type with
 * a thumb while looking at the text above them.
 *
 * The suffix rather than the first four: the prefix is derived from the words
 * of the capture, so two notes about groceries begin the same way. The suffix
 * is what makes a slug unique, which is exactly the property this needs.
 */
@Composable
private fun ConfirmDestroy(note: NoteSummary, onDismiss: () -> Unit, onGo: () -> Unit) {
    var typed by remember { mutableStateOf("") }
    val wanted = note.slug.takeLast(CONFIRM_CHARS)

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Destroy this content?") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(note.slug, style = MaterialTheme.typography.titleSmall)
                note.text?.let { Text(it.lineSequence().first()) }
                Text(
                    "This cannot be undone once the window elapses. " +
                        "Type the last $CONFIRM_CHARS characters of the name — $wanted",
                    style = MaterialTheme.typography.labelSmall,
                )
                TextField(
                    typed,
                    { typed = it },
                    Modifier.fillMaxWidth(),
                    singleLine = true,
                    // No autocorrect and no capitalisation. A slug is lowercase
                    // and a keyboard that helpfully capitalises the first
                    // letter would make the guard unclearable for reasons
                    // nobody could see.
                    keyboardOptions = KeyboardOptions(
                        autoCorrectEnabled = false,
                        capitalization = KeyboardCapitalization.None,
                    ),
                )
            }
        },
        confirmButton = {
            Button(
                onClick = onGo,
                enabled = typed.trim().equals(wanted, ignoreCase = true),
            ) { Text("Destroy") }
        },
        dismissButton = { TextButton(onDismiss) { Text("Keep it") } },
    )
}

/**
 * How much of the name destroying asks for.
 *
 * Four is a deliberate act and not a memorable one — long enough that it
 * cannot be produced without looking, short enough to type one-handed.
 */
private const val CONFIRM_CHARS = 4
