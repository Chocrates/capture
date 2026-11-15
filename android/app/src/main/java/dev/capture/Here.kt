package dev.capture

import android.annotation.SuppressLint
import android.content.Context
import com.google.android.gms.location.LocationServices
import com.google.android.gms.location.Priority
import com.google.android.gms.tasks.CancellationTokenSource

/**
 * Where this device is, once, to hang a plan on.
 *
 * A single fix rather than a subscription. The point is "next time I am here",
 * which needs one position at the moment somebody says it — holding the radio
 * open afterwards would cost battery to learn nothing, and the platform's
 * geofencing does the watching from then on.
 *
 * The result is handed to the archive as micro-degrees and never stored
 * anywhere else. A plan records that somebody chose a place; it is not a
 * record of where they have been.
 */
object Here {
    /** Whether a rough position can be read, which is all a sun time needs. */
    fun roughlyPermitted(context: Context): Boolean =
        listOf(
            android.Manifest.permission.ACCESS_COARSE_LOCATION,
            android.Manifest.permission.ACCESS_FINE_LOCATION,
        ).any {
            androidx.core.content.ContextCompat.checkSelfPermission(context, it) ==
                android.content.pm.PackageManager.PERMISSION_GRANTED
        }

    /**
     * Tell the archive where this phone is, for sunrise and sunset
     * (alarms-at-sunrise-and-sunset D5).
     *
     * The last known position first, which costs nothing; a fresh low-power
     * one only where there is none. Handed over exactly as the platform gave
     * it: the archive rounds it and decides what it means. Then the alarm is
     * re-armed, because a sun time may have moved.
     *
     * Quiet on failure. Without a position the archive says, on the alarm
     * itself, that it cannot ring until it knows where it is.
     */
    @SuppressLint("MissingPermission")
    fun forSun(context: Context) {
        if (!roughlyPermitted(context)) return
        val archive = Store.of(context)
        if (!runCatching { archive.hasSunTimes() }.getOrDefault(false)) return
        val tell = { fix: android.location.Location ->
            runCatching { archive.setHere(fix.latitude, fix.longitude, fix.time) }
            Alarms.rearm(context)
        }
        val client = LocationServices.getFusedLocationProviderClient(context)
        runCatching {
            client.lastLocation
                .addOnSuccessListener { last ->
                    if (last != null) {
                        tell(last)
                    } else {
                        client.getCurrentLocation(
                            Priority.PRIORITY_BALANCED_POWER_ACCURACY,
                            CancellationTokenSource().token,
                        ).addOnSuccessListener { fresh -> if (fresh != null) tell(fresh) }
                    }
                }
        }
    }

    @SuppressLint("MissingPermission")
    fun once(context: Context, then: (Int, Int) -> Unit, otherwise: (String) -> Unit) {
        if (!Places.permitted(context)) {
            otherwise("This device is not allowed to see where it is.")
            return
        }
        LocationServices.getFusedLocationProviderClient(context)
            .getCurrentLocation(Priority.PRIORITY_HIGH_ACCURACY, CancellationTokenSource().token)
            .addOnSuccessListener { fix ->
                if (fix == null) {
                    // A null fix is the ordinary answer indoors or with the
                    // radio cold, and it is not an error — but a plan made
                    // against a position nobody has is a plan about nowhere.
                    otherwise("Could not get a position just now. Try again outside.")
                } else {
                    then((fix.latitude * 1e6).toInt(), (fix.longitude * 1e6).toInt())
                }
            }
            .addOnFailureListener { otherwise(it.message ?: "Could not get a position.") }
    }
}
