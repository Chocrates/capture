package dev.capture

import android.app.AlarmManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
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
     * How many can arrive separately before they arrive together.
     *
     * Three is a morning. Ten is a wall, and a wall gets swiped rather than
     * read — which loses every one of them, including the one that mattered.
     */
    private const val TOGETHER = 3

    /** How far ahead to look for the next moment. */
    private const val HORIZON_MS = 30L * 24 * 60 * 60 * 1000

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
     */
    fun rearm(context: Context) {
        val manager = context.getSystemService(AlarmManager::class.java) ?: return
        val archive = Store.of(context)

        val next = runCatching { archive.due(HORIZON_MS) }
            .getOrNull()
            .orEmpty()
            .firstOrNull { !it.delivered }
            ?: return

        // Something already overdue wants the alarm as soon as the platform
        // will give it, not at a time in the past — which some versions treat
        // as "immediately" and others as nonsense.
        val at = maxOf(next.atMs, System.currentTimeMillis() + 1_000)

        // Exact only where the capture named a time of day. Everything else
        // joins a batch anyway, so spending an exact alarm — and the special
        // permission one needs — on it would be paying for precision that is
        // then deliberately thrown away.
        val allowed = canBeExact(manager)
        val exact = next.timed && allowed

        // Told to the archive, which is what reports it. Written on every
        // arming rather than once, because the permission can be taken away in
        // Settings at any time and a phone that remembered the answer from
        // when it was granted would go on claiming a precision it no longer
        // has.
        runCatching { archive.setAlertsInexact(!allowed) }

        runCatching {
            if (exact) {
                manager.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, pending(context))
            } else {
                manager.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, pending(context))
            }
        }
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
                    runCatching { archive.delivered(alert.slug, alert.occ) }
                }
            }
        } else {
            for (alert in sayable) {
                if (say(context, alert)) {
                    runCatching { archive.delivered(alert.slug, alert.occ) }
                }
            }
        }

        // Whatever is next, including anything just held. An alarm at the hour
        // a hold expires is what releases it.
        rearm(context)
        scheduleHoldRelease(context, due)
    }

    /**
     * An alarm for the moment at which something held becomes sayable.
     *
     * Without this a held alert waits for the next *moment* to come due, which
     * could be days — so one held at 23:00 would arrive when something else
     * happened to fire rather than at 07:00.
     *
     * The instant comes from the archive rather than being worked out here.
     * Turning "07:00" into a moment is date arithmetic, and doing it on the
     * surface would be a second implementation of a calculation the core
     * already makes — differing at exactly the edges nobody looks at until a
     * clock goes back.
     */
    private fun scheduleHoldRelease(context: Context, due: List<Alert>) {
        val archive = Store.of(context)
        val at = due.filter { !it.delivered }
            .mapNotNull {
                val lateBy = System.currentTimeMillis() - it.atMs
                runCatching { archive.holdUntil(it.timed, false, lateBy) }.getOrNull()
            }
            .minOrNull() ?: return

        val manager = context.getSystemService(AlarmManager::class.java) ?: return
        runCatching {
            manager.setAndAllowWhileIdle(
                AlarmManager.RTC_WAKEUP,
                at,
                PendingIntent.getBroadcast(
                    context,
                    1,
                    Intent(context, Ring::class.java),
                    PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
                ),
            )
        }
    }

    /**
     * Post one line for all of them.
     *
     * Returns whether it could be said, on the same terms as [say].
     */
    private fun summarise(context: Context, alerts: List<Alert>): Boolean {
        val manager = context.getSystemService(NotificationManager::class.java) ?: return false
        if (!manager.areNotificationsEnabled()) return false
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, "Alerts", NotificationManager.IMPORTANCE_HIGH),
        )

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

        manager.notify(
            "summary".hashCode(),
            NotificationCompat.Builder(context, CHANNEL)
                .setSmallIcon(android.R.drawable.ic_dialog_info)
                .setContentTitle("${alerts.size} things were due")
                .setContentText(alerts.first().text)
                .setStyle(style)
                .setContentIntent(open)
                .setAutoCancel(true)
                .build(),
        )
        return true
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
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, "Alerts", NotificationManager.IMPORTANCE_HIGH),
        )

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

        manager.notify(
            (alert.slug + alert.occ).hashCode(),
            NotificationCompat.Builder(context, CHANNEL)
                .setSmallIcon(android.R.drawable.ic_dialog_info)
                .setContentTitle(alert.text)
                .apply { if (late) setContentText("was due earlier") }
                .setContentIntent(open)
                .setAutoCancel(true)
                .build(),
        )

        // The channel itself can be turned off independently of the
        // application, and that is a decision rather than a fault — but it
        // means this was not said, and the moment stays due.
        return manager.getNotificationChannel(CHANNEL)?.importance != NotificationManager.IMPORTANCE_NONE
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
        if (intent.action == Intent.ACTION_BOOT_COMPLETED) {
            Alarms.rearm(context)
        }
    }
}
