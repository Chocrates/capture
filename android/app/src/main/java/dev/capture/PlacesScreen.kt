package dev.capture

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.Card
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
import androidx.compose.ui.unit.dp
import uniffi.capture_ffi.Intent as Plan
import uniffi.capture_ffi.Watching

/**
 * If-then plans, and whether anything is actually watching for them.
 *
 * The second half is not a detail. §7.3: an intent that fails invisibly is
 * worse than no intent, because somebody who believes something is watching
 * stops holding it in mind — which is the entire thing this is for. So a plan
 * this device cannot see says so on its own row, every time, rather than in a
 * warning somebody dismissed once.
 */
@Composable
fun Plans(
    plans: List<Plan>,
    watching: Watching,
    onAskPermission: () -> Unit,
    onAskAlways: () -> Unit,
    onWatch: (String, String) -> Unit,
    onRetract: (String) -> Unit,
) {
    val places = plans.count { it.cueType == "location" }

    LazyColumn(Modifier.fillMaxSize()) {
        if (watching != Watching.ALWAYS) {
            item {
                Card(Modifier.fillMaxWidth().padding(16.dp)) {
                    Column(
                        Modifier.padding(16.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Text(
                            when {
                                // The trap. "While using the app" reads like a
                                // smaller permission and is none at all for
                                // this: nobody is looking at a notes app while
                                // walking into a shop.
                                watching == Watching.WHILE_OPEN && places > 0 ->
                                    "$places plan(s) about a place only fire while " +
                                        "capture is open — which is not when you " +
                                        "would be arriving anywhere."
                                watching == Watching.WHILE_OPEN ->
                                    "Plans about places will only fire while capture " +
                                        "is open."
                                places > 0 ->
                                    "$places plan(s) about a place cannot fire. " +
                                        "Nothing is watching for them."
                                else -> "This device cannot see where it is."
                            },
                            style = MaterialTheme.typography.bodyMedium,
                        )
                        Text(
                            "No coordinate is ever written down. A plan records " +
                                "that you chose a place, not where you have been.",
                            style = MaterialTheme.typography.labelSmall,
                        )
                        if (watching == Watching.NEVER) {
                            Button(onAskPermission) { Text("Allow location") }
                        } else {
                            // Android will not grant this from a dialog: it has
                            // to be chosen in Settings, which is the right
                            // amount of friction for something that watches
                            // where you are.
                            Button(onAskAlways) { Text("Allow all the time") }
                        }
                    }
                }
            }
        }

        item { NewPlace(onWatch) }

        if (plans.isEmpty()) {
            item { Empty("No plans yet.") }
        }
        items(plans) { plan ->
            Column(Modifier.fillMaxWidth().padding(16.dp, 8.dp)) {
                Text(plan.then, style = MaterialTheme.typography.bodyLarge)
                Text(
                    plan.cueMatch?.let { "${plan.cueType} · $it" } ?: plan.cueType,
                    style = MaterialTheme.typography.labelSmall,
                )
                if (!plan.observableHere) {
                    // Said on the row, not once in a banner. A device that
                    // cannot observe a cue is not broken — the intent is
                    // waiting for a device that can — but a person needs to
                    // know which of those two they are looking at.
                    Text(
                        "This device cannot watch for this.",
                        style = MaterialTheme.typography.labelSmall,
                    )
                }
                TextButton({ onRetract(plan.id) }) { Text("Give it up") }
            }
        }
    }
}

/**
 * Make a plan about here.
 *
 * "Here" rather than a map, because a map is a milestone of its own and the
 * useful case is standing somewhere and saying "next time I am here".
 */
@Composable
private fun NewPlace(onWatch: (String, String) -> Unit) {
    var then by remember { mutableStateOf("") }
    var radius by remember { mutableStateOf("100") }

    Column(
        Modifier.fillMaxWidth().padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text("Next time I am here", style = MaterialTheme.typography.titleSmall)
        TextField(
            then,
            { then = it },
            Modifier.fillMaxWidth(),
            singleLine = true,
            label = { Text("What to say") },
        )
        TextField(
            radius,
            { radius = it },
            Modifier.fillMaxWidth(),
            singleLine = true,
            label = { Text("How close counts, in metres") },
        )
        Button({ onWatch(then.trim(), radius.trim()) }, enabled = then.isNotBlank()) {
            Text("Watch for it")
        }
    }
}
