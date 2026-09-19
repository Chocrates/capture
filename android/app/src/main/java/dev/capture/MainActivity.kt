package dev.capture

import android.Manifest
import android.app.AlarmManager
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import android.content.pm.PackageManager
import androidx.core.content.ContextCompat
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.List
import androidx.compose.material.icons.filled.DateRange
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.Place
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp

/**
 * The reading surface.
 *
 * Separate from the capture window on purpose. Capture is a gesture that has to
 * be faster than opening anything; reading is somewhere you go. Putting them in
 * one activity would mean the fast path paid for the slow one's startup.
 */
class MainActivity : ComponentActivity() {

    private lateinit var model: Model

    /**
     * Asking to see where we are.
     *
     * Only ever in response to somebody pressing the button on the plans
     * screen. A permission prompt at first launch, before anything has
     * explained why, is the one most often refused permanently — and this
     * feature does not exist until somebody wants a plan about a place.
     */
    private val askLocation = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) { _ ->
        // What was actually granted, asked of the platform rather than read
        // from the result: "while using the app" comes back as a grant.
        model.located(Places.watching(this))
        Places.rearm(this)

        // The next moment a capture named. Re-armed on every launch for the
        // same reason as the fences above: an alarm does not survive a reboot,
        // and one that silently stopped existing looks exactly like a system
        // with nothing to say.
        Alarms.rearm(this)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        model = Model(Store.of(this))
        model.go(Screen.Timeline)

        // Asked for once and then owned by the platform. A phone cannot keep a
        // daemon, so the scheduling `capture-syncd` does with a timer is done
        // by WorkManager under constraints it enforces.
        SyncWorker.keepSyncing(this)

        // Re-registered on every launch. Geofences do not survive a reboot,
        // and a fence that silently stopped existing is the failure §7.3 is
        // about: nothing is watching, and the plan still looks fine.
        Places.rearm(this)

        // The next moment a capture named. Re-armed on every launch for the
        // same reason as the fences above: an alarm does not survive a reboot,
        // and one that silently stopped existing looks exactly like a system
        // with nothing to say.
        Alarms.rearm(this)

        setContent {
            CaptureTheme {
                Surface(Modifier.fillMaxSize()) {
                    App(model, onAskLocation = {
                        askLocation.launch(
                            arrayOf(
                                Manifest.permission.ACCESS_FINE_LOCATION,
                                Manifest.permission.POST_NOTIFICATIONS,
                            ),
                        )
                    })
                }
            }
        }
    }

    override fun onResume() {
        super.onResume()
        // Whatever came due while the app was away. The desktop settles on
        // every command that writes; a phone is opened rather than invoked, so
        // this is where it happens.
        model.settle()
        // The permission can be taken away in Settings while the app is in the
        // background, so it is asked of the platform on every return rather
        // than remembered from when it was granted.
        model.located(Places.watching(this))
        model.go(model.screen)

        // After settling, because settling is what promotes a capture into the
        // thing that carries a time. Arming before it would schedule for an
        // archive that had not yet read what it holds.
        Alarms.rearm(this)

        // Asked once there is something to say, and not before.
        //
        // The permission was declared from the start and requested only
        // alongside location, which meant an archive full of dated captures
        // could not speak unless somebody had also set up a place cue. The
        // alarm fired, the notification was posted, and the platform dropped
        // it — `importance=NONE`, no error anywhere.
        //
        // Asked here rather than at first launch because §2.4.1's argument
        // holds: a permission requested before anything has explained why is
        // the one most often refused permanently. By the time something is
        // scheduled, what it is for is whatever the person just wrote down.
        // One at a time, and permission to speak first.
        //
        // These were called back to back, and the second broke the first:
        // launching the Settings activity for exact alarms takes the
        // application out of the foreground, which dismisses the notification
        // permission dialog before anybody can answer it. The result was a
        // phone that had been sent to Settings to make its alerts *precise*
        // while still not being allowed to make any.
        //
        // Notifications are required for an alert to exist at all; exact
        // alarms only decide whether it lands on its moment or within a
        // window. Asking for the optional one in a way that destroys the
        // required one is backwards.
        if (!askToSpeakIfNeeded()) {
            askForExactAlarmsIfNeeded()
        }

        // A notification tapped, rather than the app opened.
        intent?.getStringExtra(OPEN_NOTE)?.let { slug ->
            intent.removeExtra(OPEN_NOTE)
            model.go(Screen.Note(slug))
        }
    }

    /**
     * Ask for permission to speak, where there is something to say.
     *
     * Silent when nothing is scheduled: an archive with no dated captures has
     * no reason to want this, and asking anyway is how the request gets
     * refused before it means anything.
     */
    private fun askToSpeakIfNeeded(): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return false
        val granted = ContextCompat.checkSelfPermission(
            this,
            Manifest.permission.POST_NOTIFICATIONS,
        ) == PackageManager.PERMISSION_GRANTED
        if (granted || asked) return false

        val pending = runCatching { model.hasAlerts() }.getOrDefault(false)
        if (!pending) return false

        asked = true
        askNotifications.launch(Manifest.permission.POST_NOTIFICATIONS)
        return true
    }

    /**
     * The trip to Settings for exact alarms, offered once.
     *
     * `SCHEDULE_EXACT_ALARM` is not a prompt — from Android 13 it is a switch
     * in Settings, and an application can only ask to be taken there. So this
     * is an offer rather than a request, made once per launch and only where
     * something time-critical is actually scheduled.
     *
     * Refusing is a real answer and costs nothing catastrophic: the alarm
     * falls back to inexact, the health surface says the times are
     * approximate, and nothing pretends otherwise.
     */
    private fun askForExactAlarmsIfNeeded() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return
        if (offeredExact) return

        // Only once there is permission to speak at all. Being sent to
        // Settings to make silent alerts punctual is not a useful trip.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(
                this,
                Manifest.permission.POST_NOTIFICATIONS,
            ) != PackageManager.PERMISSION_GRANTED
        ) {
            return
        }

        val manager = getSystemService(AlarmManager::class.java) ?: return
        if (manager.canScheduleExactAlarms()) return

        // Only where it would make a difference. An archive whose only dated
        // things are due *dates* is batched anyway, and being sent to Settings
        // to enable a precision nothing would use is a worse experience than
        // the imprecision.
        val wantsExact = runCatching { model.hasTimedAlerts() }.getOrDefault(false)
        if (!wantsExact) return

        offeredExact = true
        Toast.short(this, "Allow alarms & reminders so times arrive exactly.")
        runCatching {
            startActivity(
                Intent(Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM)
                    .setData(Uri.fromParts("package", packageName, null)),
            )
        }
    }

    private var offeredExact = false

    /**
     * Asked at most once per launch.
     *
     * A refusal is an answer. Re-asking on every resume would turn a decision
     * into a nag, and the platform stops delivering the prompt after the
     * second refusal anyway — so the only thing repeating achieves is spending
     * the two chances that exist.
     */
    private var asked = false

    private val askNotifications = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        if (!granted) {
            Toast.short(this, "Without notifications, nothing can be said when a time arrives.")
        }
    }

    companion object {
        /** Which note a notification was about. */
        const val OPEN_NOTE = "dev.capture.open_note"
    }
}

@Composable
private fun App(model: Model, onAskLocation: () -> Unit) {
    val context = LocalContext.current

    // Back goes back, rather than out.
    //
    // Enabled only while there is somewhere to return to: at the bottom of the
    // stack the gesture belongs to the platform, and an application that
    // swallowed it there is one you cannot leave. This is also why it is not
    // simply `model.back()` unconditionally — a handler that always consumes
    // the gesture traps the person on the first screen.
    BackHandler(enabled = model.canGoBack) { model.back() }

    Scaffold(bottomBar = { Bar(model) }) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            // Said out loud rather than left as a blank list. A screen that is
            // empty because something failed is indistinguishable from a
            // screen that is empty because the archive is.
            model.trouble?.let {
                Text(it, Modifier.padding(16.dp), style = MaterialTheme.typography.bodyMedium)
            }

            when (val screen = model.screen) {
                is Screen.Timeline -> Timeline(
                    notes = model.timeline,
                    onOpen = { model.go(Screen.Note(it)) },
                    onTick = model::tick,
                )
                is Screen.Search -> Search(
                    hits = model.hits,
                    onQuery = model::search,
                    onOpen = { model.go(Screen.Note(it)) },
                    onTick = model::tick,
                )
                is Screen.Agenda -> Agenda(screen.day, model.agenda)
                is Screen.Wiki -> Wiki(model.pages) { model.go(Screen.Note(it)) }
                is Screen.Health -> Health(
                    model.conditions,
                    model.peers,
                    model.costs,
                    model.notifier,
                    model.me,
                    onNotifier = model::chooseNotifier,
                    onSync = {
                        SyncWorker.now(context)
                        // The worker is out of reach once started, so the
                        // screen is refreshed when it can next be observed to
                        // have finished: on return to this screen, and here
                        // after a moment for the ordinary quick case.
                        model.refreshSoon()
                    },
                    onPair = { model.go(Screen.Pairing) },
                )
                is Screen.Review -> ReviewSession(
                    model.offered,
                    model.nagging,
                    onOutcome = model::reviewed,
                    onOpen = { model.go(Screen.Note(it)) },
                )
                is Screen.Plans -> Plans(
                    model.plans,
                    model.located,
                    onAskPermission = onAskLocation,
                    // Background location cannot be granted from a dialog on
                    // any recent Android: the platform requires a deliberate
                    // trip to Settings, and that is the right amount of
                    // friction for something that watches where you are.
                    onAskAlways = {
                        context.startActivity(
                            android.content.Intent(
                                android.provider.Settings
                                    .ACTION_APPLICATION_DETAILS_SETTINGS,
                                android.net.Uri.fromParts("package", context.packageName, null),
                            ),
                        )
                    },
                    onWatch = { then, metres ->
                        Here.once(
                            context,
                            then = { lat, lon ->
                                model.watchPlace(
                                    lat,
                                    lon,
                                    metres.toUIntOrNull() ?: 100u,
                                    then,
                                )
                                Places.rearm(context)
                            },
                            otherwise = { Toast.short(context, it) },
                        )
                    },
                    onRetract = model::retract,
                )
                is Screen.Pairing -> Pairing(
                    model.pairing,
                    onOffer = { model.offerPairing(model.addresses()) },
                    onJoin = model::joinPairing,
                )
                is Screen.Note -> NotePage(model.note, model.pending, model.recording, model)
                is Screen.ListOf -> ListScreen(
                    tag = screen.tag,
                    lines = model.list,
                    onTick = { item, done -> model.tick(screen.tag, item, done) },
                )
            }
        }
    }
}

@Composable
private fun Bar(model: Model) {
    val here = model.screen
    NavigationBar {
        for (tab in TABS) {
            NavigationBarItem(
                selected = tab.selected(here),
                onClick = { model.go(tab.go(model)) },
                icon = { Icon(tab.icon, contentDescription = null) },
                label = { Text(tab.label) },
            )
        }
    }
}

/**
 * One destination in the bar.
 *
 * A table rather than seven hand-written items, because seven of anything
 * written out by hand is seven places to forget one — the previous version had
 * the selection rule for each spelled out separately, and `Note` counting as
 * `Timeline` was in exactly one of them.
 */
private class Tab(
    val label: String,
    val icon: ImageVector,
    val selected: (Screen) -> Boolean,
    val go: (Model) -> Screen,
)

/**
 * The seven, and why they have icons now.
 *
 * Material's `NavigationBar` is specified for three to five destinations.
 * There are seven, and they were drawn with `icon = {}` — so each label had
 * a seventh of the width to itself, which on a Pixel 8 Pro is 144dp. "Timeline"
 * wrapped mid-word to "Timelin / e" and "Health" was clipped at the right edge.
 *
 * An icon is not decoration here: it is what makes the label stop competing
 * for horizontal space, because the item lays them out vertically. The labels
 * stay, because an icon alone is a thing you have to learn and §3.1.1's whole
 * argument about the capture syntax — discovered by seeing it — applies at
 * least as much to navigation.
 *
 * Seven is still over Material's guidance, and the honest fix is fewer
 * destinations. That is a change to how the application is navigated rather
 * than to how it is drawn, and it wants somebody's opinion after a week of
 * use rather than mine now.
 */
private val TABS = listOf(
    Tab(
        // "Timeline" rather than "Notes" everywhere else, and "Notes" here.
        //
        // Icons fixed six of the seven labels; this one still wrapped to
        // "Timelin / e", because the selected tab is drawn with a pill behind
        // it that takes width the others do not need — so the longest label is
        // also the one with least room, exactly when it is being looked at.
        "Notes",
        Icons.AutoMirrored.Filled.List,
        // A note opened from the timeline is still the timeline. The bar
        // should not appear to have moved because a row was tapped.
        { it is Screen.Timeline || it is Screen.Note },
        { Screen.Timeline },
    ),
    Tab("Agenda", Icons.Filled.DateRange, { it is Screen.Agenda }, { Screen.Agenda(it.today()) }),
    Tab("Pages", Icons.Filled.Menu, { it is Screen.Wiki }, { Screen.Wiki }),
    Tab("Search", Icons.Filled.Search, { it is Screen.Search }, { Screen.Search }),
    Tab("Plans", Icons.Filled.Place, { it is Screen.Plans }, { Screen.Plans }),
    Tab("Review", Icons.Filled.Refresh, { it is Screen.Review }, { Screen.Review }),
    Tab(
        "Health",
        Icons.Filled.Favorite,
        // Pairing lives behind Health, which is where the devices are.
        { it is Screen.Health || it is Screen.Pairing },
        { Screen.Health },
    ),
)
