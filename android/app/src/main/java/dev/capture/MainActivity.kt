package dev.capture

import android.Manifest
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
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

        setContent {
            MaterialTheme {
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
    }
}

@Composable
private fun App(model: Model, onAskLocation: () -> Unit) {
    val context = LocalContext.current
    Scaffold(bottomBar = { Bar(model) }) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            // Said out loud rather than left as a blank list. A screen that is
            // empty because something failed is indistinguishable from a
            // screen that is empty because the archive is.
            model.trouble?.let {
                Text(it, Modifier.padding(16.dp), style = MaterialTheme.typography.bodyMedium)
            }

            when (val screen = model.screen) {
                is Screen.Timeline -> Timeline(model.timeline) { model.go(Screen.Note(it)) }
                is Screen.Search -> Search(model.hits, model::search) { model.go(Screen.Note(it)) }
                is Screen.Agenda -> Agenda(screen.day, model.agenda)
                is Screen.Wiki -> Wiki(model.pages) { model.go(Screen.Note(it)) }
                is Screen.Health -> Health(
                    model.conditions,
                    model.peers,
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
                is Screen.Note -> NotePage(model.note, model.pending, model)
            }
        }
    }
}

@Composable
private fun Bar(model: Model) {
    val here = model.screen
    NavigationBar {
        NavigationBarItem(
            selected = here is Screen.Timeline || here is Screen.Note,
            onClick = { model.go(Screen.Timeline) },
            icon = {},
            label = { Text("Timeline") },
        )
        NavigationBarItem(
            selected = here is Screen.Agenda,
            onClick = { model.go(Screen.Agenda(model.today())) },
            icon = {},
            label = { Text("Agenda") },
        )
        NavigationBarItem(
            selected = here is Screen.Wiki,
            onClick = { model.go(Screen.Wiki) },
            icon = {},
            label = { Text("Pages") },
        )
        NavigationBarItem(
            selected = here is Screen.Search,
            onClick = { model.go(Screen.Search) },
            icon = {},
            label = { Text("Search") },
        )
        NavigationBarItem(
            selected = here is Screen.Plans,
            onClick = { model.go(Screen.Plans) },
            icon = {},
            label = { Text("Plans") },
        )
        NavigationBarItem(
            selected = here is Screen.Review,
            onClick = { model.go(Screen.Review) },
            icon = {},
            label = { Text("Review") },
        )
        NavigationBarItem(
            // Pairing lives behind Health, which is where the devices are.
            selected = here is Screen.Health || here is Screen.Pairing,
            onClick = { model.go(Screen.Health) },
            icon = {},
            label = { Text("Health") },
        )
    }
}
