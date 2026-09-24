package dev.capture

import android.app.PendingIntent
import android.content.Intent
import android.os.Build
import android.service.quicksettings.TileService

/**
 * One pull, one tap, from the lock screen.
 *
 * The fastest surface the platform offers that does not require unlocking, and
 * therefore the one that matters most: §2.1's whole argument is that a thought
 * survives only as long as capturing it stays cheaper than holding on to it.
 * An app icon costs an unlock, a home screen, a search and a cold start.
 *
 * The tile does not capture. It opens the field. Committing from a tile with
 * no text would be a gesture with nothing in it, and a tile that captured
 * silently is one that cannot be corrected.
 */
class CaptureTile : TileService() {

    override fun onClick() {
        super.onClick()

        // NEW_TASK and nothing else. CaptureActivity declares an empty
        // taskAffinity, so it is alone in a task of its own and there is
        // never anything above it to clear; a repeat press is absorbed by
        // launchMode="singleTop" instead. CLEAR_TOP was here when the
        // activity still shared MainActivity's task, and leaving it would
        // describe a stack the manifest now forbids.
        val intent = Intent(this, CaptureActivity::class.java).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }

        // `startActivityAndCollapse` takes a PendingIntent from API 34 and an
        // Intent before it, and the older overload throws on the newer
        // platform rather than being deprecated quietly. Both are here because
        // minSdk is 30 and the tile is the one surface that must not fail
        // silently — a tile that does nothing is indistinguishable from a tile
        // that was not pressed.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startActivityAndCollapse(
                PendingIntent.getActivity(
                    this,
                    0,
                    intent,
                    PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
                )
            )
        } else {
            @Suppress("DEPRECATION")
            startActivityAndCollapse(intent)
        }
    }
}
