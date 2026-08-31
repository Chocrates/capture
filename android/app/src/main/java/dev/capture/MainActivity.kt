package dev.capture

import android.os.Bundle
import androidx.activity.ComponentActivity
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

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        model = Model(Store.of(this))
        model.go(Screen.Timeline)

        // Asked for once and then owned by the platform. A phone cannot keep a
        // daemon, so the scheduling `capture-syncd` does with a timer is done
        // by WorkManager under constraints it enforces.
        SyncWorker.keepSyncing(this)

        setContent {
            MaterialTheme {
                Surface(Modifier.fillMaxSize()) { App(model) }
            }
        }
    }

    override fun onResume() {
        super.onResume()
        // Whatever came due while the app was away. The desktop settles on
        // every command that writes; a phone is opened rather than invoked, so
        // this is where it happens.
        model.settle()
        model.go(model.screen)
    }
}

@Composable
private fun App(model: Model) {
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
                    onSync = { SyncWorker.now(context) },
                    onPair = { model.go(Screen.Pairing) },
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
            selected = here is Screen.Health,
            onClick = { model.go(Screen.Health) },
            icon = {},
            label = { Text("Health") },
        )
    }
}
