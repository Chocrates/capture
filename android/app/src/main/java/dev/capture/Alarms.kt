package dev.capture

import android.app.AlarmManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.media.RingtoneManager
import android.os.Build
import androidx.core.app.NotificationCompat
import uniffi.capture_ffi.Alert

/**
 * Saying something when a moment a capture named arrives.
 *
 * # Why AlarmManager and not a worker
 *
 * `WorkManager`'s minimum periodic interval is fifteen minutes and doze
 * stretches it further, so "a meeting starting" would arrive at some point in
 * the following half hour. A capture that named 2pm meant 2pm, and a system
 * that treats that as a suggestion is one somebody stops trusting for the only
 * thing it was for.
 *
 * # One alarm, not one per moment
 *
 * Only the next moment is scheduled. A phone holding a year of dated captures
 * would otherwise hold a year of alarms, and the platform caps how many an
 * application may have — a cap you discover by silently losing the ones past
 * it. Each firing schedules the next, which also means a corrected time is
 * picked up without anything having to cancel an alarm already set.
 */
object Alarms {

    const val CHANNEL = "alerts"

    /**
     * Where a moment with a time of day is said: loudly, and until answered.
     *
     * Asked for on 2026-10-13 — "louder and more obnoxious". The `alerts`
     * channel played one default notification sound with vibration off, so a
     * phone on vibrate said nothing at all about a 2pm appointment.
     *
     * A new id rather than changing `alerts`, because a channel's sound and
     * vibration are fixed once it exists. The alarm stream follows the alarm
     * volume, which is up when the ringer is not, and plays through vibrate.
     * A day-only moment stays on [CHANNEL]: a task due today announcing itself
     * like an alarm at every batch is pressure, not a reminder.
     */
    const val LOUD = "alarms"

    private fun channels(manager: NotificationManager) {
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, "Alerts", NotificationManager.IMPORTANCE_HIGH),
        )
        manager.createNotificationChannel(
            NotificationChannel(LOUD, "Timed alerts", NotificationManager.IMPORTANCE_HIGH).apply {
                description = "Something with a time: before it, and when it starts."
                setSound(
                    RingtoneManager.getDefaultUri(RingtoneManager.TYPE_ALARM)
                        ?: RingtoneManager.getDefaultUri(RingtoneManager.TYPE_NOTIFICATION),
                    AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_ALARM)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                        .build(),
                )
                enableVibration(true)
                vibrationPattern = longArrayOf(0, 600, 300, 600, 300, 600)
                lockscreenVisibility = Notification.VISIBILITY_PUBLIC
            },
        )
    }

    /**
     * Repeat the sound until somebody dismisses or opens it.
     *
     * One ring is what was missed. `FLAG_INSISTENT` is the platform's own
     * "until answered", and it stops on swipe, tap or a newer post with the
     * same id — so the moment itself, posted under its warning's id, replaces
     * a warning nobody answered rather than ringing over it.
     */
    private fun insistent(n: Notification): Notification =
        n.apply { flags = flags or Notification.FLAG_INSISTENT }

    /**
     * How many can arrive separately before they arrive together.
     *
     * Three is a morning. Ten is a wall, and a wall gets swiped rather than
     * read — which loses every one of them, including the one that mattered.
     */
    private const val TOGETHER = 3

    /** How far ahead to look for the next moment. */
    private const val HORIZON_MS = 30L * 24 * 60 * 60 * 1000

    /**
     * How long to wait before asking again after the archive could not be read.
     *
     * Five minutes: comfortably past a sync round's window, which is the
     * contention this exists for, and far enough apart that a fault which is
     * not transient costs a handful of wakes a day rather than a loop.
     */
    private const val RETRY_MS = 5L * 60 * 1000

    private fun pending(context: Context): PendingIntent = PendingIntent.getBroadcast(
        context,
        0,
        Intent(context, Ring::class.java),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )

    /**
     * Schedule whatever comes next, replacing whatever was scheduled.
     *
     * Called after every capture, on every resume, on boot, and by the alarm
     * itself once it has fired. Cheap, and safe to call when nothing is
     * pending: the archive answers with nothing and no alarm is set.
     *
     * `soonestMs` is how long to wait for something that is due already. A
     * second after a capture or on resume; [RETRY_MS] after a ring, because
     * anything still due then is something the ring could not post, and asking
     * again a second later is the wake loop with extra steps.
     */
    /**
     * Whether this device can speak at all, recorded on every arming and every
     * return to the application — not only when a ring tries and fails.
     *
     * Only the ring recorded it, so a phone with notifications switched off
     * reported nothing wrong until an alert had already been dropped: the one
     * moment telling somebody is too late to help.
     */
    fun recordCanSpeak(context: Context) {
        runCatching {
            Store.of(context).setAlertsSilenced(
                context.getSystemService(NotificationManager::class.java)
                    ?.areNotificationsEnabled() != true,
            )
        }
    }

    fun rearm(context: Context, soonestMs: Long = 1_000) {
        val manager = context.getSystemService(AlarmManager::class.java) ?: return
        val archive = Store.of(context)

        recordCanSpeak(context)

        // Whether this device is the one that speaks (§2.4.1).
        //
        // Asked here as well as at delivery, because arming is what costs: a
        // device that is not going to announce anything has no reason to wake
        // for a moment the other one is handling. A failure to ask is treated
        // as yes — silence is the expensive mistake, and the setting is a
        // preference about where an alert lands rather than whether it exists.
        if (!runCatching { archive.saysThingsHere() }.getOrDefault(true)) {
            runCatching { archive.setAlertsUnscheduled(false) }
            return
        }

        // Asked as three outcomes, not two.
        //
        // This was `runCatching { ... }.getOrNull().orEmpty().firstOrNull { ... }
        // ?: return`, which collapses "the archive says nothing is due" into
        // "the archive could not be read". The first correctly sets no alarm.
        // The second ends the chain: only one alarm is ever outstanding and each
        // firing schedules the next, so an arming that gives up schedules
        // nothing, and the firing that would have tried again is the one that
        // was never set. Alerts then stop for good — until the owner happens to
        // open the application.
        //
        // It is not a rare path. `due()` opens a Session, which takes the
        // writer flock and refuses when a sync round holds it (`session.rs`),
        // and flock conflicts between file descriptions in the same process —
        // so an alarm firing while a round is mid-exchange is exactly this.
        //
        // *When* is the archive's answer, not the first undelivered moment. A
        // moment held to morning or too old to say is undelivered on purpose,
        // and arming for it a second from now rang, held it again, and armed
        // again: 20,105 wakes in an hour and a half on a Pixel, from one
        // invoice due a fortnight before. `nextWake` asks each moment the way
        // the ring will, so a hold is armed for its end and a stale moment for
        // nothing.
        val wake = runCatching { archive.nextWake(HORIZON_MS, soonestMs) }
        val next = wake.getOrNull()

        val at = when {
            next != null -> next.atMs

            wake.isFailure ->
                // Could not read the archive. Keep the chain alive and ask
                // again: the contention this comes from is a sync round holding
                // the writer lock for a bounded window of seconds, so by the
                // retry it is long over. Minutes rather than seconds because a
                // failure that is not contention should not become a wake loop.
                System.currentTimeMillis() + RETRY_MS

            // Read it, and there is genuinely nothing to say. No alarm, and the
            // chain is not broken by its absence — a capture, a resume or a
            // boot will arm the next one.
            else -> {
                runCatching { archive.setAlertsUnscheduled(false) }
                return
            }
        }

        val allowed = canBeExact(manager)

        // Exact only where the capture named a time of day, and only for a real
        // moment — a retry is this code asking itself a question again and has
        // no precision worth spending a permission on.
        val exact = next != null && next.exact && allowed

        // Set first, recorded afterwards, and recorded from what happened.
        //
        // The flag used to be written before the attempt, which made the one
        // case it exists for report the opposite of the truth: the exact-alarm
        // permission is revoked in Settings between `canBeExact` and the call,
        // `setExactAndAllowWhileIdle` throws, and `alerts.inexact = false` has
        // already been written. The surface then states that alerts are exact
        // while no alarm exists at all.
        //
        // So an exact alarm that is refused falls back to an inexact one — late
        // is a great deal better than never — and what gets reported is whichever
        // one is actually set.
        var scheduled = false
        var inexact = !allowed

        if (exact) {
            val ok = runCatching {
                manager.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, pending(context))
            }
            scheduled = ok.isSuccess
            if (ok.isFailure) inexact = true
        }

        if (!scheduled) {
            scheduled = runCatching {
                manager.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, pending(context))
            }.isSuccess
        }

        runCatching { archive.setAlertsInexact(inexact) }

        // The chain's own state, said out loud. If neither call would take an
        // alarm there is nothing left to try and nothing that will try again,
        // and that is worth a condition on the health surface rather than a
        // swallowed exception.
        runCatching { archive.setAlertsUnscheduled(!scheduled) }
    }

    /**
     * Whether this device will honour an exact alarm.
     *
     * `SCHEDULE_EXACT_ALARM` is granted in Settings from Android 13 rather than
     * by a prompt, and asking for one that has not been granted throws. Checked
     * rather than assumed, and the answer is reported on the health surface —
     * a cue that fires at the wrong time is worse than one that says it cannot
     * be sure.
     */
    fun canBeExact(manager: AlarmManager): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.S || manager.canScheduleExactAlarms()

    /**
     * Deliver everything that has come due, subject to the budget.
     *
     * The decision about *whether* this may be said now belongs to the archive
     * (§2.6), not here: a phone with its own idea of quiet hours would be a
     * second implementation of a rule that has to agree with the desktop's, and
     * the two would drift the first time either changed.
     */
    fun ring(context: Context) {
        val archive = Store.of(context)
        if (!runCatching { archive.saysThingsHere() }.getOrDefault(true)) return
        val due = runCatching { archive.due(0) }.getOrNull().orEmpty()

        // Whether this device can speak at all, recorded every time it tries.
        //
        // The platform's way of refusing a notification is to accept it and
        // drop it, so this is the only moment the answer is known — and
        // without recording it, an archive full of scheduled moments would sit
        // silent with nothing anywhere saying why.
        val manager = context.getSystemService(NotificationManager::class.java)
        runCatching {
            archive.setAlertsSilenced(manager?.areNotificationsEnabled() != true)
        }

        val now = System.currentTimeMillis()
        val sayable = due.filter { alert ->
            !alert.delivered &&
                // Held rather than said: quiet hours, or a batch it belongs
                // to. Nothing is marked delivered, so it is still due when
                // the hour comes round.
                runCatching {
                    archive.holdUntil(alert.timed, false, now - alert.atMs)
                }.getOrNull() == null
        }

        // Marked delivered only if it actually was.
        //
        // Posting to a blocked channel succeeds and does nothing: the platform
        // drops it and reports no error. Marking it anyway meant the archive
        // recorded "said" for something nobody could have heard — and because
        // delivery happens once, the moment was then gone for good.
        if (sayable.size > TOGETHER) {
            // One notification, not nine.
            //
            // A phone left alone overnight comes back holding a day of
            // moments, and posting each of them is a wall somebody clears by
            // swiping rather than by reading — which loses every one of them,
            // including the one that mattered. That is §2.6's own argument
            // arrived at from the other direction.
            if (summarise(context, sayable)) {
                for (alert in sayable) {
                    runCatching { archive.delivered(alert.slug, alert.occ, alert.atMs) }
                }
            }
        } else {
            for (alert in sayable) {
                if (say(context, alert)) {
                    runCatching { archive.delivered(alert.slug, alert.occ, alert.atMs) }
                }
            }
        }

        // Whatever is next, including anything just held — `nextWake` arms
        // for the hour a hold ends, which is what releases it. Anything still
        // due now is something this ring could not post, so it waits minutes.
        rearm(context, RETRY_MS)
    }

    /**
     * Post one line for all of them.
     *
     * Returns whether it could be said, on the same terms as [say].
     */
    private fun summarise(context: Context, alerts: List<Alert>): Boolean {
        val manager = context.getSystemService(NotificationManager::class.java) ?: return false
        if (!manager.areNotificationsEnabled()) return false
        channels(manager)
        // Loud if any of them named a time: one of them is happening.
        val channel = if (alerts.any { it.timed }) LOUD else CHANNEL

        val open = PendingIntent.getActivity(
            context,
            "summary".hashCode(),
            Intent(context, MainActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        // The texts themselves, not a count. "9 things were due" says nothing
        // anybody can act on; the first few lines say whether to open it.
        val style = NotificationCompat.InboxStyle()
        for (a in alerts.take(5)) style.addLine(a.text)
        if (alerts.size > 5) style.setSummaryText("and ${alerts.size - 5} more")

        val built = NotificationCompat.Builder(context, channel)
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setContentTitle("${alerts.size} things were due")
            .setContentText(alerts.first().text)
            .setStyle(style)
            .setContentIntent(open)
            .setAutoCancel(true)
            .build()
        manager.notify(
            "summary".hashCode(),
            if (channel == LOUD) insistent(built) else built,
        )

        // The same check `say` ends with, and it was missing here — which is
        // the worse place for it to be missing. The caller marks *every* alert
        // in the batch delivered on the strength of this return, so a muted
        // channel would have spent nine moments on one notification nobody
        // could see. Delivery happens once.
        return manager.getNotificationChannel(channel)?.importance !=
            NotificationManager.IMPORTANCE_NONE
    }

    /**
     * Post one, and say whether it could be.
     *
     * The return is load-bearing: the caller only records a delivery that
     * happened. `notify` on a blocked channel is not an error — it is a
     * silence — so whether this device may speak has to be asked rather than
     * assumed from the call succeeding.
     */
    private fun say(context: Context, alert: Alert): Boolean {
        val manager = context.getSystemService(NotificationManager::class.java) ?: return false
        if (!manager.areNotificationsEnabled()) return false
        channels(manager)
        val channel = if (alert.timed) LOUD else CHANNEL

        // Late is said, not hidden. A phone that was off through an afternoon
        // should say the afternoon happened rather than presenting a moment
        // from three hours ago as if it were now.
        val late = System.currentTimeMillis() - alert.atMs > 5 * 60 * 1000

        val open = PendingIntent.getActivity(
            context,
            alert.slug.hashCode(),
            Intent(context, MainActivity::class.java)
                .putExtra(MainActivity.OPEN_NOTE, alert.slug),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )

        val built = NotificationCompat.Builder(context, channel)
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setContentTitle(alert.text)
            .apply {
                when {
                    // A warning says how far ahead. Not "late": a warning
                    // whose moment has begun is never handed over.
                    alert.aheadMin > 0u -> setContentText("in ${alert.aheadMin} min")
                    late -> setContentText("was due earlier")
                    alert.timed -> setContentText("now")
                }
            }
            .setCategory(if (alert.timed) NotificationCompat.CATEGORY_ALARM else NotificationCompat.CATEGORY_REMINDER)
            .setPriority(NotificationCompat.PRIORITY_MAX)
            .setContentIntent(open)
            .setAutoCancel(true)
            .build()

        // One id for a moment and its warning, so the moment replaces the
        // warning (and stops it ringing) rather than stacking under it.
        manager.notify(
            (alert.slug + alert.occ).hashCode(),
            if (channel == LOUD) insistent(built) else built,
        )

        // The channel itself can be turned off independently of the
        // application, and that is a decision rather than a fault — but it
        // means this was not said, and the moment stays due.
        return manager.getNotificationChannel(channel)?.importance != NotificationManager.IMPORTANCE_NONE
    }
}

/** The alarm arriving. */
class Ring : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        Alarms.ring(context)
    }
}

/**
 * A restart.
 *
 * Alarms do not survive one, so without this a phone that rebooted overnight
 * would stop alerting and give no sign of it — the worst shape of failure for
 * something whose whole job is to speak up.
 */
class Restarted : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            Intent.ACTION_BOOT_COMPLETED,
            Intent.ACTION_TIMEZONE_CHANGED,
            Intent.ACTION_TIME_CHANGED,
            -> {}
            else -> return
        }

        // Which zone this device is in, reported rather than worked out.
        //
        // Android exposes neither `$TZ` nor `/etc/localtime`, so the core's own
        // lookup returns whatever the library read when the process started and
        // keeps returning it. The platform is the only thing that knows, and
        // this is the moment it says so.
        runCatching {
            Store.of(context).setZone(java.util.TimeZone.getDefault().id)
        }

        // And the alarm is recomputed, because the instant it was set for was
        // worked out in the old zone. A clock change without this leaves an
        // alarm pointing at a moment that is no longer the moment it named.
        Alarms.rearm(context)

        // Geofences do not survive a restart either, and this receiver only
        // restored the alarms.
        //
        // The fences were re-registered on every launch of MainActivity, which
        // sounds sufficient and is not: the surfaces that matter are the tile
        // and the share sheet, and somebody who captures through those can go
        // days without opening the application. A phone that rebooted
        // overnight would then have no place cues at all, while the health
        // surface went on reporting that location was permitted — which it
        // was. Permission is not registration.
        runCatching { Places.rearm(context) }
    }
}
