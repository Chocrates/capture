package dev.capture

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Card
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import uniffi.capture_ffi.Resurfaced

/**
 * Notes coming back round.
 *
 * There is no score here and there will not be one. §2.7 removed the graded
 * outcome rather than the feature, because a recurring, indefinitely
 * repeating, permanently recorded history of memory failures — delivered on a
 * schedule the person did not set, to somebody whose stated difficulty is
 * memory — is the one thing this component could do that would cause harm.
 *
 * So there are two buttons and neither is a mark.
 */
@Composable
fun ReviewSession(
    items: List<Resurfaced>,
    nagging: List<String>,
    onOutcome: (String, String) -> Unit,
    onOpen: (String) -> Unit,
) {
    if (items.isEmpty() && nagging.isEmpty()) {
        Empty("Nothing to look at again yet.")
        return
    }

    LazyColumn(Modifier.fillMaxSize()) {
        items(items) { item ->
            Card(Modifier.fillMaxWidth().padding(16.dp, 8.dp)) {
                Column(
                    Modifier.padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    // Why this one, on the card. A resurfacing whose reason is
                    // invisible is indistinguishable from a random one.
                    Text(
                        when (item.because) {
                            "forward" -> "You said you'd come back to this"
                            "never-engaged" -> "Captured, never returned to"
                            else -> "Fits where you are now"
                        },
                        style = MaterialTheme.typography.labelSmall,
                    )
                    Text(
                        item.text ?: "(no readable text)",
                        style = MaterialTheme.typography.bodyLarge,
                    )
                    item.suggestion?.let {
                        // A reading the archive had and did not act on. Said as
                        // a question rather than applied, because the whole
                        // reason it is here is that it was not sure enough.
                        Text(it, style = MaterialTheme.typography.labelSmall)
                    }
                    Row {
                        TextButton({ onOpen(item.slug) }) { Text("Open") }
                        TextButton({ onOutcome(item.slug, "engaged") }) { Text("Did something") }
                        TextButton({ onOutcome(item.slug, "deferred") }) { Text("Not now") }
                    }
                }
            }
        }

        if (nagging.isNotEmpty()) {
            item {
                HorizontalDivider(Modifier.padding(vertical = 16.dp))
                Text(
                    "Keeps slipping",
                    Modifier.padding(horizontal = 16.dp),
                    style = MaterialTheme.typography.titleSmall,
                )
                Text(
                    "Named rather than counted: \"three things keep slipping\" " +
                        "is not something anybody can act on.",
                    Modifier.padding(horizontal = 16.dp),
                    style = MaterialTheme.typography.labelSmall,
                )
            }
            items(nagging) { slug ->
                TextButton({ onOpen(slug) }, Modifier.padding(horizontal = 8.dp)) { Text(slug) }
            }
        }
    }
}
