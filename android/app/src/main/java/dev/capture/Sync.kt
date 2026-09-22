package dev.capture

import android.content.Context
import android.net.wifi.WifiManager
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Sync, on a phone that is not allowed to keep a daemon.
 *
 * The desktop runs `capture-syncd` and decides for itself when to reconcile.
 * Android does not permit that, and pretending otherwise produces an
 * application that syncs while it is open and never again — which is the worst
 * of both, because it looks like it works.
 *
 * So the platform decides when, under constraints it enforces, and the round
 * itself is the same Rust the daemon runs.
 */
class SyncWorker(context: Context, params: WorkerParameters) :
    CoroutineWorker(context, params) {

    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        val wifi = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager

        // The trap the design names by name (§5.2).
        //
        // Android drops multicast packets before they reach the socket unless
        // this lock is held. There is no error and no packet: mDNS browse runs,
        // waits, and reports that nobody is there — indistinguishable from a
        // network with nobody on it. Acquiring the lock costs battery, which is
        // exactly why the platform makes it opt-in and why it is released in
        // `finally` rather than at some later convenient moment.
        val lock = wifi.createMulticastLock("capture-sync").apply {
            setReferenceCounted(false)
            acquire()
        }

        try {
            val report = Store.of(applicationContext).sync(WINDOW_SECONDS)
            if (!report.multicast) {
                // Said rather than swallowed. A round that found nothing
                // because multicast was unavailable is a different situation
                // from a round that found nothing because nobody was awake,
                // and only one of them is worth doing something about.
                Toast.note(applicationContext, "sync ran without discovery")
            }
            Result.success()
        } catch (e: Exception) {
            // Retried with the platform's backoff rather than this one's. A
            // peer that was asleep is the ordinary case and not worth a
            // notification.
            Result.retry()
        } finally {
            if (lock.isHeld) lock.release()
        }
    }

    companion object {
        /**
         * How long a round is given.
         *
         * Long enough to complete a handshake and an exchange over a slow
         * link; short enough to leave room inside the ten minutes WorkManager
         * allows before it kills the job outright.
         */
        const val WINDOW_SECONDS = 20u

        private const val PERIODIC = "capture-sync"

        /**
         * Ask for sync to keep happening.
         *
         * Fifteen minutes is the platform's floor for periodic work, not a
         * choice. Doze will stretch it, sometimes to hours, and that is the
         * behaviour to design for rather than fight: it is why every surface
         * reports when a peer was last reached instead of implying currency.
         */
        fun keepSyncing(context: Context) {
            val work = PeriodicWorkRequestBuilder<SyncWorker>(15, TimeUnit.MINUTES)
                .setConstraints(
                    Constraints.Builder()
                        // Connected, not unmetered.
                        //
                        // This asked for UNMETERED to avoid spending somebody's
                        // cellular data on moving a whole log. The intent was
                        // right and the constraint did not express it: "metered"
                        // is a flag on a *network*, not a statement about
                        // cellular, and a home Wi-Fi carries it whenever it is a
                        // phone hotspot, some carrier-supplied routers, or the
                        // owner once tapped "Network usage → Metered". On such a
                        // network UNMETERED is never satisfied, so the job is
                        // never eligible — and an ineligible job is not a failed
                        // one. It does not run, does not retry and does not
                        // report, all week, while every surface looks correct.
                        //
                        // Cellular is not the exposure it was guarding anyway.
                        // A round discovers peers by mDNS on the local network
                        // and otherwise dials an address it remembers, which is
                        // a LAN address: off that LAN it is unroutable and the
                        // dial fails in the first second. There is no path by
                        // which this spends mobile data on a log.
                        .setRequiredNetworkType(NetworkType.CONNECTED)
                        .build(),
                )
                .build()

            WorkManager.getInstance(context).enqueueUniquePeriodicWork(
                PERIODIC,
                // UPDATE rather than KEEP.
                //
                // KEEP was chosen so that re-enqueuing on every launch would not
                // reset the period — an application opened often would otherwise
                // sync less than one opened rarely. That reasoning is correct and
                // UPDATE keeps it: it preserves the existing period and its next
                // run time, and changes only the specification.
                //
                // What KEEP also did was make the constraints above permanent on
                // every device that had already installed once. A phone that
                // enqueued this work with UNMETERED keeps obeying UNMETERED
                // forever, and no later build can reach it — the corrected
                // request is discarded, unread, in favour of the one that does
                // not work. A bug in a WorkSpec is not shippable under KEEP.
                ExistingPeriodicWorkPolicy.UPDATE,
                work,
            )
        }

        /** Sync now, because somebody asked. */
        fun now(context: Context) {
            WorkManager.getInstance(context)
                .enqueue(OneTimeWorkRequestBuilder<SyncWorker>().build())
        }
    }
}
