package dev.capture

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import uniffi.capture_ffi.NoteSummary

/**
 * A note's text, written full screen (pages, 2026-10-13).
 *
 * What the one-line Edit dialog was standing in for: a page longer than a
 * sentence could not be written on the phone at all. Multi-line, with the
 * keyboard's own suggestions (this is prose), and completion after `[[`, `@`,
 * `^` and `#` from the same read the desktop's editor uses — so writing a link
 * never means going to find a slug to copy.
 *
 * Saving writes a new version and keeps the old one. Leaving with changes
 * asks first, because the back gesture is easy to make by accident.
 */
@Composable
fun Editor(note: NoteSummary?, model: Model) {
    if (note == null) {
        Empty("Loading.")
        return
    }
    val original = note.text ?: ""
    var value by remember(note.slug) {
        mutableStateOf(TextFieldValue(original, TextRange(original.length)))
    }
    var offers by remember { mutableStateOf<List<String>>(emptyList()) }
    var leaving by remember { mutableStateOf(false) }
    val dirty = value.text != original

    BackHandler { if (dirty) leaving = true else model.leaveEditor(note.slug) }

    // What is being completed: the word the caret is in, if it starts with a
    // sigil. Read as the text changes; the archive is asked off the main thread.
    val open = openSigil(value)
    LaunchedEffect(open) {
        offers = if (open == null) emptyList() else model.complete(open.sigil, open.prefix)
    }

    Column(Modifier.fillMaxSize().imePadding()) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            TextButton({ if (dirty) leaving = true else model.leaveEditor(note.slug) }) {
                Text("Cancel")
            }
            Text(note.alias ?: note.slug, style = MaterialTheme.typography.titleSmall)
            Button({ model.saveText(note.slug, value.text) }, enabled = dirty) { Text("Save") }
        }
        HorizontalDivider()
        OutlinedTextField(
            value,
            { value = it },
            Modifier.fillMaxWidth().weight(1f).padding(8.dp),
            textStyle = MaterialTheme.typography.bodyLarge,
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
            placeholder = {
                Text("Write. [[ links a page, @ a person, ^ a project, # a tag; \"- \" starts a checklist line.")
            },
        )
        // Above the keyboard, where the thumb already is.
        if (open != null && offers.isNotEmpty()) {
            Row(
                Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(8.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                for (name in offers) {
                    AssistChip(
                        onClick = { value = complete(value, open, name) },
                        label = { Text(open.sigil + name) },
                    )
                }
            }
        }
    }

    if (leaving) {
        AlertDialog(
            onDismissRequest = { leaving = false },
            title = { Text("Discard your changes?") },
            text = { Text("What you wrote since opening this will be lost. The page keeps its last saved version.") },
            confirmButton = {
                TextButton({ leaving = false; model.leaveEditor(note.slug) }) { Text("Discard") }
            },
            dismissButton = { Button({ leaving = false }) { Text("Keep writing") } },
        )
    }
}

/** A sigil being typed: which one, where it starts, and what follows it. */
private data class Open(val sigil: String, val at: Int, val prefix: String)

/**
 * The completion the caret is in, if any.
 *
 * `[[` runs to the caret unless closed or broken by a line; the one-character
 * sigils run back to the start of the word. Narrow on purpose, so text that
 * merely contains brackets somewhere earlier never traps the editor in a
 * completion — the same rule the desktop's editor keeps.
 */
private fun openSigil(v: TextFieldValue): Open? {
    if (!v.selection.collapsed) return null
    val upto = v.text.substring(0, v.selection.start)
    val link = upto.lastIndexOf("[[")
    if (link >= 0) {
        val after = upto.substring(link + 2)
        if (!after.contains("]]") && !after.contains('\n')) return Open("[[", link, after)
    }
    val word = upto.takeLastWhile { !it.isWhitespace() }
    val sigil = word.firstOrNull()?.toString() ?: return null
    if (sigil !in listOf("@", "^", "#")) return null
    val prefix = word.drop(1)
    // A tag's list form (`#groceries:`) is finished once its colon is typed.
    if (prefix.contains(':')) return null
    return Open(sigil, upto.length - word.length, prefix)
}

/** The text with the open sigil replaced by the chosen name, caret after it. */
private fun complete(v: TextFieldValue, open: Open, name: String): TextFieldValue {
    val before = v.text.substring(0, open.at)
    val after = v.text.substring(v.selection.start)
    val inserted = if (open.sigil == "[[") "[[$name]]" else "${open.sigil}$name "
    val text = before + inserted + after
    return TextFieldValue(text, TextRange(before.length + inserted.length))
}
