package dev.capture

import android.Manifest
import android.app.AlarmManager
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import uniffi.capture_ffi.Watching

/**
 * What the platform lets this application do, in one place a person can see.
 *
 * # Why this exists
 *
 * Every permission here was asked for only at the moment something needed it,
 * which is right for a first ask and wrong for every ask after. Notifications
 * were requested once there was something to say; exact alarms were offered
 * once notifications had been granted and something timed was scheduled. A
 * phone that refused the first prompt, or upgraded from a build that never
 * asked, had no way back: the alerts were silent, the times were approximate,
 * and nothing anywhere offered to change either. Found on a Pixel 8 Pro after
 * an update, with both refused and no route to grant them short of digging
 * through the system's own settings.
 *
 * So each one is listed with whether it is granted, what it costs to be
 * without it, and a button that does the platform's version of asking — a
 * prompt where there is one, the system's settings page where there is not.
 */
enum class Permit {
    NOTIFICATIONS,
    EXACT_ALARMS,
    LOCATION,
    LOCATION_ALWAYS,
    APPROXIMATE_LOCATION,
    MICROPHONE,
}

/** One permission, as the settings screen shows it. */
data class PermitState(
    val permit: Permit,
    val name: String,
    val granted: Boolean,
    /** What is lost without it, said plainly. */
    val without: String,
)

object Permissions {

    /**
     * Every permission and whether it is held, asked of the platform.
     *
     * Asked on every return rather than remembered: any of these can be taken
     * away in the system's settings while the application is in the background.
     */
    fun states(context: Context): List<PermitState> {
        val granted = { p: String ->
            ContextCompat.checkSelfPermission(context, p) == PackageManager.PERMISSION_GRANTED
        }
        val watching = Places.watching(context)
        val exact = context.getSystemService(AlarmManager::class.java)
            ?.let { Alarms.canBeExact(it) } ?: false

        return listOf(
            PermitState(
                Permit.NOTIFICATIONS,
                "Notifications",
                Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
                    granted(Manifest.permission.POST_NOTIFICATIONS),
                "Nothing is said when a time arrives. The platform drops the alert without an error.",
            ),
            PermitState(
                Permit.EXACT_ALARMS,
                "Alarms & reminders",
                exact,
                "Times arrive within a window rather than on the minute — and a sleeping phone can stretch that window a long way.",
            ),
            PermitState(
                Permit.LOCATION,
                "Location",
                watching != Watching.NEVER,
                "Plans about places cannot be made.",
            ),
            PermitState(
                Permit.LOCATION_ALWAYS,
                "Location, all the time",
                watching == Watching.ALWAYS,
                "A place plan only fires while this app is open — which, walking into a shop, it never is.",
            ),
            PermitState(
                Permit.APPROXIMATE_LOCATION,
                "Approximate location",
                Here.roughlyPermitted(context),
                "Sunrise and sunset alarms cannot ring: the time depends on where you are.",
            ),
            PermitState(
                Permit.MICROPHONE,
                "Microphone",
                granted(Manifest.permission.RECORD_AUDIO),
                "Hold-to-talk cannot record.",
            ),
        )
    }
}

/** The permissions, at the top of the settings screen. */
@Composable
fun PermissionsSection(states: List<PermitState>, onAsk: (Permit) -> Unit) {
    Column(Modifier.fillMaxWidth()) {
        Text("Permissions", style = MaterialTheme.typography.titleSmall)
        for (s in states) {
            Row(
                Modifier.fillMaxWidth().padding(vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text(s.name, style = MaterialTheme.typography.bodyMedium)
                    Text(
                        if (s.granted) "Allowed" else s.without,
                        style = MaterialTheme.typography.labelSmall,
                    )
                }
                // Granted ones have no button. Taking a permission back is the
                // system's settings, and offering it here would be a control
                // that does not do what it says.
                if (!s.granted) {
                    TextButton({ onAsk(s.permit) }) { Text("Allow") }
                }
            }
        }
    }
}
