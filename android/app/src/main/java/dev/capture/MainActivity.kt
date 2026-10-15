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
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.DateRange
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.Settings
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
import androidx.compose.runtime.mutableStateOf
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
     * What the platform allows, as the settings screen shows it.
     *
     * Asked again on every resume and after every answer, because each of
     * these can be changed in the system's settings while this is away.
     */
    private val permits = mutableStateOf<List<PermitState>>(emptyList())

    private fun refreshPermits() {
        permits.value = Permissions.states(this)
    }

    /** A single runtime prompt, from the settings screen. */
    private val askOne = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        // Refused with "don't ask again", the platform answers no without
        // showing anything — so a button that only prompted would do nothing
        // at all, which is the failure this screen exists to end. Where the
        // prompt cannot appear, the system's page for this app can.
        //
        // Told apart by how fast the answer came. A refusal somebody made took
        // them at least a glance at a dialog; one the platform made took
        // milliseconds. The first version went to Settings whenever nothing
        // had changed — so pressing "Don't allow" also threw the person into
        // the system's settings, overriding the answer they had just given.
        val unseen = System.currentTimeMillis() - askedAt < UNSEEN_MS
        refreshPermits()
        if (!granted && unseen) openAppSettings()
        Alarms.rearm(this)
    }

    /** When the last prompt was launched, to tell a dialog from no dialog. */
    private var askedAt = 0L

    private fun openAppSettings() {
        runCatching {
            startActivity(
                Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS)
                    .setData(Uri.fromParts("package", packageName, null)),
            )
        }
    }

    /** Do the platform's version of asking for one permission. */
    private fun ask(permit: Permit) {
        when (permit) {
            Permit.NOTIFICATIONS ->
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    askedAt = System.currentTimeMillis()
                    askOne.launch(Manifest.permission.POST_NOTIFICATIONS)
                }
            // Not a prompt on any recent Android: a switch in Settings, which
            // an application may only open.
            Permit.EXACT_ALARMS -> runCatching {
                startActivity(
                    Intent(Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM)
                        .setData(Uri.fromParts("package", packageName, null)),
                )
            }
            Permit.LOCATION -> askLocation.launch(
                arrayOf(
                    Manifest.permission.ACCESS_FINE_LOCATION,
                    Manifest.permission.POST_NOTIFICATIONS,
                ),
            )
            // Background location cannot be granted from a dialog: the
            // platform requires a deliberate trip to Settings.
            Permit.LOCATION_ALWAYS -> openAppSettings()
            Permit.MICROPHONE -> {
                askedAt = System.currentTimeMillis()
                askOne.launch(Manifest.permission.RECORD_AUDIO)
            }
        }
    }

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
        refreshPermits()
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
        // Opens on what is outstanding, not on the log. See the first entry
        // in TABS for why.
        model.go(Screen.Tasks)

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
                    App(model, permits.value, ::ask, onAskLocation = {
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
        refreshPermits()
        // Before the screen is loaded, so a banner reflects what the platform
        // allows now rather than what it allowed at the last ring.
        Alarms.recordCanSpeak(this)
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
        /**
         * Faster than anybody answers a dialog, slower than the platform
         * answers for one it will not show.
         */
        private const val UNSEEN_MS = 500L

        /** Which note a notification was about. */
        const val OPEN_NOTE = "dev.capture.open_note"
    }
}

@Composable
private fun App(
    model: Model,
    permits: List<PermitState>,
    onPermit: (Permit) -> Unit,
    onAskLocation: () -> Unit,
) {
    val context = LocalContext.current

    // Back goes back, rather than out.
    //
    // Enabled only while there is somewhere to return to: at the bottom of the
    // stack the gesture belongs to the platform, and an application that
    // swallowed it there is one you cannot leave. This is also why it is not
    // simply `model.back()` unconditionally — a handler that always consumes
    // the gesture traps the person on the first screen.
    BackHandler(enabled = model.canGoBack) { model.back() }

    // No tabs while writing: on a phone held sideways with the keyboard up,
    // the bar left the editor a sliver of a line (seen 2026-10-13).
    Scaffold(bottomBar = { if (model.screen !is Screen.Editor) Bar(model) }) { padding ->
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
                    onTick = model::tickAt,
                )
                is Screen.Search -> Search(
                    hits = model.hits,
                    onQuery = model::search,
                    onOpen = { model.go(Screen.Note(it)) },
                    onTick = model::tickAt,
                )
                is Screen.Agenda -> Agenda(
                    screen.day,
                    model,
                    onTasks = { model.go(Screen.Tasks) },
                    onOpen = { model.go(Screen.Note(it)) },
                )
                is Screen.Tasks -> Tasks(
                    tasks = model.tasks,
                    unread = model.unread,
                    onOpen = { model.go(Screen.Note(it)) },
                    onDone = model::setDone,
                    onAgenda = { model.go(Screen.Agenda(model.today())) },
                    attention = model.conditions.filter { it.tier == "attention" },
                    onAttention = { model.go(Screen.Health) },
                )
                is Screen.Wiki -> Wiki(
                    pages = model.pages,
                    people = model.people,
                    projects = model.projects,
                    onOpen = { model.go(Screen.Note(it)) },
                    onEntity = { model.go(Screen.Entity(it)) },
                    onNewPage = model::newPage,
                )
                is Screen.Entity -> EntityPage(
                    mention = screen.mention,
                    entity = model.entity,
                    onOpen = { model.go(Screen.Note(it)) },
                    model = model,
                )
                is Screen.Health -> Health(
                    model.conditions,
                    model.peers,
                    model.costs,
                    model.notifier,
                    model.defaultNotifier,
                    model.me,
                    onNotifier = model::chooseNotifier,
                    onRetire = model::retire,
                    onSync = { model.syncNow(context) },
                    onPair = { model.go(Screen.Pairing) },
                    permits = permits,
                    onPermit = onPermit,
                    leadMin = model.leadMin,
                    onLead = { model.setLead(context, it) },
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
                is Screen.Editor -> Editor(model.note, model)
                is Screen.ListOf -> ListScreen(
                    tag = screen.tag,
                    lines = model.list,
                    onTick = { slug, ord, done -> model.tickAt(slug, ord, done) },
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
    // First, and the application opens here.
    //
    // The timeline held this place and should not have. It is the log with
    // everything in it, in the order things were written — which is the right
    // shape for an archive and the wrong one for a landing surface, because
    // "what have I got outstanding" is not answerable by scrolling what you
    // happened to write most recently.
    //
    // One tab for both halves of §3.6, opening on the half asked more often.
    // Not an eighth tab: the row is already one past what this width holds and
    // the note below says what that costs. The agenda is one tap from this
    // screen's header, and cannot answer for anything undated anyway — an
    // undated task has no day to appear on.
    //
    // They stay separate screens. Folding one into the other re-conflates what
    // occupies time with what merely has state, which is what §3.6's table
    // exists to keep apart.
    Tab(
        "Tasks",
        Icons.Filled.CheckCircle,
        { it is Screen.Tasks || it is Screen.Agenda },
        { Screen.Tasks },
    ),
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
        { it is Screen.Timeline || it is Screen.Note || it is Screen.Editor },
        { Screen.Timeline },
    ),
    // Pages holds people and projects too, so an entity page keeps this tab
    // selected rather than appearing to have navigated away from it.
    Tab(
        "Pages",
        Icons.Filled.Menu,
        { it is Screen.Wiki || it is Screen.Entity },
        { Screen.Wiki },
    ),
    Tab("Search", Icons.Filled.Search, { it is Screen.Search }, { Screen.Search }),
    Tab("Plans", Icons.Filled.Place, { it is Screen.Plans }, { Screen.Plans }),
    Tab("Review", Icons.Filled.Refresh, { it is Screen.Review }, { Screen.Review }),
    // "Settings" rather than "Health" since permissions joined it: what the
    // platform allows is the thing somebody goes looking for, and a tab named
    // for diagnostics is not where they look. The health conditions are still
    // here, below the permissions.
    Tab(
        "Settings",
        Icons.Filled.Settings,
        // Pairing lives behind Health, which is where the devices are.
        { it is Screen.Health || it is Screen.Pairing },
        { Screen.Health },
    ),
)
