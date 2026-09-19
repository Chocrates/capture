package dev.capture

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
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
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp

/**
 * Connecting this phone to another device.
 *
 * Both directions, because either device may be the one with a screen in front
 * of the person. A phone that could only ever join would make the desktop the
 * senior device, and §0.2 says they are the same archive seen from two places.
 *
 * Nothing here is a preference. Pairing writes no `setting` (§8.9) — the
 * temptation is specific and worth naming: the first phone to join is an
 * obvious notifier, and recording that would mean somebody's notifier was
 * chosen by the order they happened to pair things in.
 */
@Composable
fun Pairing(state: PairingState, onOffer: () -> Unit, onJoin: (String, String) -> Unit) {
    Column(
        Modifier.fillMaxSize().padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        when (state) {
            is PairingState.Idle -> Idle(onOffer, onJoin)
            is PairingState.Trying -> {
                Text("Reaching the other device…", style = MaterialTheme.typography.titleSmall)
                Text(
                    "If this does not finish: the other device needs `capture pair` " +
                        "running, both have to be on the same network, and a VPN on " +
                        "this phone will route the other one into a tunnel it cannot " +
                        "come back from.",
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            is PairingState.Showing -> Showing(state)
            is PairingState.Joined -> Text("Paired with ${state.key}. It will receive the archive on the next sync.")
            is PairingState.Failed -> {
                Text("Pairing did not finish.", style = MaterialTheme.typography.titleSmall)
                // The reason, not a shrug. Pairing fails for reasons a person
                // can act on — the wrong address, a code that has expired, a
                // device that is not on this network — and each wants a
                // different next move.
                Text(state.why, style = MaterialTheme.typography.bodyMedium)
                TextButton(onOffer) { Text("Try again") }
            }
        }
    }
}

@Composable
private fun Idle(onOffer: () -> Unit, onJoin: (String, String) -> Unit) {
    var addr by remember { mutableStateOf("") }
    var code by remember { mutableStateOf("") }

    Text("Show a code", style = MaterialTheme.typography.titleSmall)
    Text(
        "Type it on the other device: capture pair CODE --at <this phone>",
        style = MaterialTheme.typography.bodySmall,
    )
    Button(onOffer) { Text("Show a code") }

    Text(
        "Or type a code",
        style = MaterialTheme.typography.titleSmall,
        modifier = Modifier.padding(top = 16.dp),
    )
    Text(
        "Run capture pair on the other device, then type what it shows.",
        style = MaterialTheme.typography.bodySmall,
    )
    TextField(
        addr,
        { addr = it },
        Modifier.fillMaxWidth(),
        singleLine = true,
        label = { Text("Where it is, e.g. 192.168.1.40") },
    )
    TextField(
        code,
        { code = it },
        Modifier.fillMaxWidth(),
        singleLine = true,
        label = { Text("The six digits") },
    )
    Button(
        { onJoin(addr.trim(), code.trim()) },
        enabled = addr.isNotBlank() && code.trim().length == 6,
    ) { Text("Join") }
}

@Composable
private fun Showing(state: PairingState.Showing) {
    Text("On the other device, run:", style = MaterialTheme.typography.bodyMedium)
    Text(
        "capture pair ${state.code} --at ${state.address ?: "this phone"}",
        style = MaterialTheme.typography.bodySmall,
    )
    Text(
        state.code,
        Modifier.fillMaxWidth().padding(vertical = 24.dp),
        style = MaterialTheme.typography.displayMedium,
        textAlign = TextAlign.Center,
    )
    // Said plainly, because it is the security property rather than a detail:
    // a million online guesses is a different proposition from a million
    // offline ones, and only while the code stands still.
    Text(
        "Waiting. The code stops working after one wrong attempt, " +
            "and expires by itself.",
        style = MaterialTheme.typography.bodySmall,
    )
}

/** Where a pairing has got to. */
sealed interface PairingState {
    data object Idle : PairingState

    /**
     * Dialling the other device.
     *
     * Missing until it cost an hour. Joining opens a socket and runs a
     * handshake over it, and against an unreachable address that blocks until
     * the platform gives up — which on a phone with a VPN routing the LAN into
     * a tunnel is a long time. The screen stayed on `Idle` throughout: no
     * spinner, no message, nothing. Pressing Join did nothing, over and over,
     * while the thing it started was working perfectly on an address that
     * could not answer.
     *
     * §7.3 is about exactly this shape — something that fails invisibly is
     * worse than something that fails, because the person is left debugging
     * their own typing.
     */
    data object Trying : PairingState

    data class Showing(val code: String, val address: String?) : PairingState
    data class Joined(val key: String) : PairingState
    data class Failed(val why: String) : PairingState
}
