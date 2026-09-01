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
