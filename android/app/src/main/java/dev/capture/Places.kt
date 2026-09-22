package dev.capture

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import com.google.android.gms.location.Geofence
import com.google.android.gms.location.GeofencingClient
import com.google.android.gms.location.GeofencingEvent
import com.google.android.gms.location.GeofencingRequest
import com.google.android.gms.location.LocationServices
import uniffi.capture_ffi.Watching

/**
 * Plans about places.
 *
 * §2.4.1 puts location cues on the phone for a reason that is not load
 * balancing: a geofence answered by a machine that does not move either never
 * fires or fires once and never stops. And it fires *here* whatever device is
 * the notifier — routing "you are at the hardware shop" to a desktop at home
 * delivers it to an empty room.
 *
 * The platform decides only that a boundary was crossed. What that means, what
 * to say, and whether saying it again so soon would be noise are all decided in
 * the archive.
 */
object Places {

    /**
     * How much of the time this device may see where it is.
     *
     * Three states, because the middle one is a trap. Android's "while using
     * the app" reads like a smaller permission and is none at all for a
     * geofence: nobody is looking at a notes application while walking into a
     * hardware shop. A fence registered under it is accepted, reported as
     * working, and never fires — §7.3's failure wearing the disguise of having
     * worked, which is exactly how it appeared on the emulator.
     */
    fun watching(context: Context): Watching {
        val granted = { p: String ->
            ContextCompat.checkSelfPermission(context, p) == PackageManager.PERMISSION_GRANTED
        }
        return when {
            !granted(Manifest.permission.ACCESS_FINE_LOCATION) -> Watching.NEVER
            granted(Manifest.permission.ACCESS_BACKGROUND_LOCATION) -> Watching.ALWAYS
            else -> Watching.WHILE_OPEN
        }
    }

    /** Whether a position can be read at all, which is less than a fence needs. */
    fun permitted(context: Context): Boolean = watching(context) != Watching.NEVER

    /**
     * Register every place the archive is watching for, and tell it whether we
     * are allowed to watch at all.
     *
     * The telling matters as much as the watching. An intent that cannot fire
     * has to keep being reported (§7.3) — somebody who believes something is
     * watching for them stops holding it in mind, which is the entire thing
     * this is for.
     */
    fun rearm(context: Context) {
        val archive = Store.of(context)
        val allowed = watching(context)
        archive.locationPermitted(allowed)

        // Registered under WHILE_OPEN too, deliberately. The fences are not
        // wrong — they simply will not be delivered in the background — and
        // removing them would mean a person who later grants the background
        // permission has nothing registered until they happen to reopen the
        // app. Health says what state they are in; this keeps them ready.
        if (allowed == Watching.NEVER) return

        val fences = archive.geofences()
        if (fences.isEmpty()) return

        val client: GeofencingClient = LocationServices.getGeofencingClient(context)
        val built = fences.map { f ->
            Geofence.Builder()
                .setRequestId(f.id)
                // Micro-degrees back to degrees. The archive keeps integers
                // because a float would go into an event id; the platform
                // wants degrees, and this is the one place they meet.
                .setCircularRegion(f.latE6 / 1e6, f.lonE6 / 1e6, f.radiusM.toFloat())
                .setExpirationDuration(Geofence.NEVER_EXPIRE)
                .setTransitionTypes(Geofence.GEOFENCE_TRANSITION_ENTER)
                .build()
        }

        val request = GeofencingRequest.Builder()
            // Not INITIAL_TRIGGER_ENTER: registering a fence around where you
            // already are would fire it immediately, so every restart would
            // announce that you have arrived somewhere you never left.
            .setInitialTrigger(0)
            .addGeofences(built)
            .build()

        try {
            client.addGeofences(request, pending(context))
                .addOnFailureListener {
                    // Registration can fail for reasons that have nothing to
                    // do with permission — location services switched off, too
                    // many fences, Play Services unavailable — and every one of
                    // them leaves plans that look registered and are not. The
                    // result of this call was ignored in the first version,
                    // which made it precisely the failure §7.3 is about.
                    archive.locationPermitted(Watching.NEVER)
                    Toast.note(context, "plans about places could not be registered")
                }
        } catch (e: SecurityException) {
            // Revoked between the check and the call. Recorded rather than
            // swallowed, so health reports the plans as unwatched.
            archive.locationPermitted(Watching.NEVER)
        }
    }

    /** Where the platform sends a crossing. */
    private fun pending(context: Context): PendingIntent = PendingIntent.getBroadcast(
        context,
        0,
        Intent(context, Arrival::class.java),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_MUTABLE,
    )

    const val CHANNEL = "cues"

    /**
     * Say something, because a cue came true — and say whether it could be.
     *
     * The return is load-bearing. A place cue cannot be retried: it is held
     * for half an hour afterwards, and standing inside the fence produces no
     * further entry transition, so an arrival spent on a notification the
     * platform dropped is a visit gone. The caller marks the cue fired only
     * on a `true`.
     */
    fun say(context: Context, text: String): Boolean {
        val manager = context.getSystemService(NotificationManager::class.java)
            ?: return false
        if (!manager.areNotificationsEnabled()) return false
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, "Cues", NotificationManager.IMPORTANCE_HIGH),
        )

        val open = PendingIntent.getActivity(
            context,
            0,
            Intent(context, MainActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        manager.notify(
            text.hashCode(),
            NotificationCompat.Builder(context, CHANNEL)
                .setSmallIcon(android.R.drawable.ic_dialog_info)
                .setContentTitle(text)
                .setContentIntent(open)
                .setAutoCancel(true)
                .build(),
        )

        // The channel can be muted on its own, without the application being
        // silenced — and posting to a muted channel succeeds and does nothing.
        return manager.getNotificationChannel(CHANNEL)?.importance !=
            NotificationManager.IMPORTANCE_NONE
    }
}

/**
 * A boundary was crossed.
 *
 * This receiver knows a fence id and nothing else. Whether that means anything
 * — whether the intent still exists, whether it fired so recently that saying
 * it again would be noise — is the archive's to decide, and `arrived` both
 * answers and records, so a receiver that forgot to mark it could not produce
 * a notification every few minutes.
 */
class Arrival : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val event = GeofencingEvent.fromIntent(intent) ?: return
        if (event.hasError()) return

        val archive = Store.of(context)
        for (fence in event.triggeringGeofences.orEmpty()) {
            val say = archive.arrived(fence.requestId) ?: continue
            // Spent only if it was heard. `arrived` no longer marks the cue
            // fired; this does, and only on a delivery that happened.
            if (Places.say(context, say)) {
                runCatching { archive.cueDelivered(fence.requestId) }
            }
        }
    }
}
