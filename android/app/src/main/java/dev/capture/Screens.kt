package dev.capture

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Checkbox
import androidx.compose.material3.Card
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.foundation.text.ClickableText
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import uniffi.capture_ffi.AgendaEntry
import uniffi.capture_ffi.Condition
import uniffi.capture_ffi.ListLine
import uniffi.capture_ffi.Named
import uniffi.capture_ffi.NoteSummary
import uniffi.capture_ffi.Peer
import uniffi.capture_ffi.Pending
import uniffi.capture_ffi.Recording
import uniffi.capture_ffi.TaskBand
import uniffi.capture_ffi.TaskLine
import uniffi.capture_ffi.Entity

/**
 * The timeline: everything, newest first, grouped by the day it belongs to.
 *
 * Grouped by the `day` the archive supplied, never by one computed here. A
 * note captured at 23:40 in one place must not move to a different day when
 * read in another (§2.3), and deciding that on this side would be a second
 * answer to a question that already has one.
 */
@Composable
fun Timeline(
    notes: List<NoteSummary>,
    onOpen: (String) -> Unit,
    onTick: (String, UInt, Boolean) -> Unit,
) {
    if (notes.isEmpty()) {
        Empty("Nothing captured yet.")
        return
    }
    LazyColumn(Modifier.fillMaxSize()) {
        // A fold rather than groupBy, to keep the order the archive gave us:
        // sorting by day here would re-decide the ordering.
        var lastDay: String? = null
        items(notes) { note ->
            if (note.day != lastDay) {
                lastDay = note.day
                Text(
                    note.day,
                    style = MaterialTheme.typography.labelLarge,
                    fontWeight = FontWeight.Bold,
                    modifier = Modifier.padding(start = 16.dp, top = 16.dp, bottom = 4.dp),
                )
            }
            NoteRow(note, onOpen, onTick)
        }
    }
}

/**
 * How many items of a list a timeline row draws before it says how many are
 * left.
 *
 * A number arrived at by looking rather than reasoned out: uncapped, a
 * twelve-item shopping list filled most of a phone screen by itself.
 */
private const val ITEMS_ON_A_ROW = 5

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun NoteRow(
    note: NoteSummary,
    onOpen: (String) -> Unit,
    onTick: (String, UInt, Boolean) -> Unit,
) {
    Column(
        Modifier.fillMaxWidth().clickable { onOpen(note.slug) }.padding(16.dp, 8.dp)
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(note.at, style = MaterialTheme.typography.labelSmall)
            Text(note.alias ?: note.slug, style = MaterialTheme.typography.labelSmall)
            // What the archive made of it, where it made anything. Empty for an
            // ordinary note, because a note's content is its text and inventing
            // a label for one would add a fact the capture did not contain.
            if (note.descriptor.isNotEmpty()) {
                Text(
                    note.descriptor,
                    style = MaterialTheme.typography.labelSmall,
                    fontWeight = FontWeight.Bold,
                )
            }
        }

        // The items, where this capture names any. Drawn on the row rather than
        // behind a tap: the syntax is an accelerant for writing, and a reader
        // left to decode `#groceries: milk, eggs, bread` by eye is doing the
        // recognizer's job themselves.
        //
        // Capped, because the uncapped version was measured against a real
        // list and a twelve-item shop took 1200 of a Pixel 8 Pro's 2244
        // pixels — one capture, most of a screen, everything else below the
        // fold. A timeline is a view of a day; a row that hides the day is a
        // worse answer than a row that says how much it is not showing.
        //
        // Five rather than three: four items is an ordinary short list and
        // truncating one of those would be noise. The whole list is one tap
        // away on the note, which is where a list somebody is working through
        // belongs anyway.
        for (item in note.items.take(ITEMS_ON_A_ROW)) {
            Row(
                Modifier.fillMaxWidth()
                    // Tap ticks, hold opens.
                    //
                    // An item row fills most of a list capture, so tapping one
                    // was the only thing a thumb could reach — and the note,
                    // where editing a typo lives, was only reachable by
                    // hitting the timestamp or the source line underneath. The
                    // first attempt at opening one ticked the milk off.
                    //
                    // The tick keeps the whole row because that is the gesture
                    // made one-handed in a shop, and shrinking it to the
                    // checkbox would make the common action the harder one. A
                    // hold is the standard way to reach the thing behind a row
                    // and costs the quick gesture nothing.
                    .combinedClickable(
                        // By position. The row knows which item it drew, and
                        // the words cannot say when two of them read the same —
                        // which is exactly what a shopping list does.
                        onClick = { item.ord?.let { onTick(note.slug, it, !item.done) } },
                        onLongClick = { onOpen(note.slug) },
                    )
                    .padding(vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Checkbox(checked = item.done, onCheckedChange = null)
                Text(
                    item.text,
                    style = MaterialTheme.typography.bodyLarge,
                    textDecoration = if (item.done) TextDecoration.LineThrough else null,
                    modifier = Modifier.padding(start = 12.dp),
                )
            }
        }

        if (note.items.size > ITEMS_ON_A_ROW) {
            Text(
                "and ${note.items.size - ITEMS_ON_A_ROW} more",
                style = MaterialTheme.typography.labelMedium,
                modifier = Modifier.padding(start = 12.dp, top = 4.dp, bottom = 4.dp),
            )
        }

        Text(
            // Absence is meaningful and is not an empty string. A withheld note
            // says which of the two states it is in, because one of them can
            // still be cancelled and the other cannot.
            note.text ?: when {
                note.redacted -> "(destroyed)"
                note.redactionQueued -> "(queued for destruction — can still be cancelled)"
                else -> "(no readable text)"
            },
            // Secondary once there is a rendering above it, and the whole row
            // when there is not. §3.1.4 keeps it on screen either way: the text
            // saying "every day at 7" beside fields saying weekdays is the
            // disagreement a reader has to be able to catch.
            style = if (note.items.isEmpty() && note.descriptor.isEmpty()) {
                MaterialTheme.typography.bodyLarge
            } else {
                MaterialTheme.typography.bodySmall
            },
        )
    }
    HorizontalDivider()
}

/** One note in full, and everything that can be done to it. */
@Composable
fun NotePage(note: NoteSummary?, pending: Pending?, recording: Recording?, model: Model) {
    if (note == null) {
        Empty("Loading.")
        return
    }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        Column(
            Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(note.alias ?: note.slug, style = MaterialTheme.typography.titleMedium)
            Text("${note.day} ${note.at}", style = MaterialTheme.typography.labelSmall)
            // What the archive made of it. §3.1.4 wants this beside the text
            // rather than instead of it: the text saying "every day at 7"
            // against fields saying weekdays is a disagreement only a reader
            // can settle, and only if both are on screen.
            if (note.descriptor.isNotEmpty()) {
                Text(
                    note.descriptor,
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.Bold,
                )
            }

            val body = note.text
            if (body == null) {
                Text(
                    when {
                        note.redacted -> "Destroyed. The content is gone."
                        note.redactionQueued -> "Withheld while the destruction is pending."
                        else -> "(no readable text)"
                    },
                    style = MaterialTheme.typography.bodyLarge,
                )
            } else {
                // The mentions in it lead to their pages. §4.3 calls the entity
                // link the one that carries the load — it exists without anybody
                // having linked anything — and rendering it as inert text here
                // while the window makes it a link would make one note mean two
                // different things depending on where it is read.
                MentionedText(body, model)
            }
            // The items this capture names, drawn here rather than behind a
            // tap. The button that used to stand in this place existed because
            // the row could not render itself; a row that can does not need
            // one.
            for (item in note.items) {
                Row(
                    Modifier.fillMaxWidth()
                        // By position, not by words. The row knows which one it
                        // is, and two items reading the same used to be
                        // untickable from here for want of saying so.
                        .clickable {
                            item.ord?.let { model.tickAt(note.slug, it, !item.done) }
                        }
                        .padding(vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Checkbox(checked = item.done, onCheckedChange = null)
                    Text(
                        item.text,
                        style = MaterialTheme.typography.bodyLarge,
                        textDecoration = if (item.done) TextDecoration.LineThrough else null,
                        modifier = Modifier.padding(start = 12.dp),
                    )
                }
            }

            // The tag-wide view still answers the wider question — everything
            // under this tag, across every capture that named some.
            note.listTag?.let { tag ->
                TextButton({ model.go(Screen.ListOf(tag)) }) {
                    Text("Everything on #$tag")
                }
            }
            if (note.mutedUntilMs != null) {
                Text("Muted.", style = MaterialTheme.typography.labelMedium)
            }
            if (note.pinned) Text("Pinned.", style = MaterialTheme.typography.labelMedium)
            // A reading the archive was not confident enough to act on.
            //
            // Offered rather than applied, which is what the confidence floor
            // is for: below it the guesses are the ones most likely to be
            // wrong, and a wrong promotion costs a calendar entry where a
            // wrong suggestion costs a glance. Accepting is the other half of
            // that bargain — the person reading this wrote the text, and is
            // better placed than the parser to say whether it meant Tuesday.
            model.suggestion?.let { offered ->
                Text(offered, style = MaterialTheme.typography.labelMedium)
                TextButton({ model.accept(note.slug) }) { Text("Yes, that's what it is") }
            }
            recording?.let { Listen(it, model) }
        }
        HorizontalDivider()
        Actions(note, pending, model)
    }
}

/**
 * A recording, and whether it is here to be played.
 *
 * "Not on this device" is an ordinary state, not a fault: a phone evicts
 * aggressively (§5.6) and the bytes come back on the next round with a device
 * that still holds them. Saying that is different from saying the recording is
 * missing, and a person deleting the note because it looked broken would be
 * acting on the difference.
 */
@Composable
private fun Listen(recording: Recording, model: Model) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var playing by remember { mutableStateOf(false) }

    Column {
        Text(
            recording.durationMs
                ?.let { "A recording, ${it / 1000u} seconds" }
                ?: "A recording",
            style = MaterialTheme.typography.labelMedium,
        )
        val gone = recording.unavailableForDays
        if (!recording.here && gone != null) {
            // Not "no device has it": only that none this phone has reached
            // does. A laptop shut for a week holding the only copy looks
            // exactly like this, so the words leave room for it.
            Text(
                "Not on any device this phone has reached in " +
                    (if (gone <= 1u) "the last day" else "$gone days") +
                    ". If the device that recorded it no longer has it, it " +
                    "will not arrive.",
                style = MaterialTheme.typography.labelSmall,
            )
        } else if (!recording.here) {
            Text(
                "Not on this device yet. It will arrive on the next sync with " +
                    "a device that has it.",
                style = MaterialTheme.typography.labelSmall,
            )
        } else {
            TextButton({
                if (playing) {
                    Playback.stop()
                    playing = false
                } else {
                    scope.launch {
                        val bytes = model.bytesOf(recording.name)
                        if (bytes == null) {
                            // Held a moment ago and not now: evicted between
                            // the read and the tap, which is unlikely and not
                            // impossible.
                            Toast.short(context, "Those bytes are no longer on this device.")
                        } else {
                            playing = true
                            Playback.play(context, bytes) { playing = false }
                        }
                    }
                }
            }) { Text(if (playing) "Stop" else "Play") }
        }
    }
}

@Composable
fun Search(
    hits: List<NoteSummary>,
    onQuery: (String) -> Unit,
    onOpen: (String) -> Unit,
    onTick: (String, UInt, Boolean) -> Unit,
) {
    var query by remember { mutableStateOf("") }
    Column(Modifier.fillMaxSize()) {
        TextField(
            value = query,
            onValueChange = { query = it; onQuery(it) },
            modifier = Modifier.fillMaxWidth().padding(16.dp),
            singleLine = true,
        )
        if (query.isNotBlank() && hits.isEmpty()) Empty("Nothing matches.")
        LazyColumn { items(hits) { NoteRow(it, onOpen, onTick) } }
    }
}

/**
 * What is on a day.
 *
 * Three bands, and the first is not a subdivision of the second: an all-day
 * entry is a floating date with no position on an hour axis, and a task that is
 * *due* on a day is a third thing again (§4.4, §3.6).
 */
@Composable
fun Agenda(day: String, entries: List<AgendaEntry>, onTasks: () -> Unit) {
    Column(Modifier.fillMaxSize()) {
        Row(
            Modifier.fillMaxWidth().padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(day, style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.weight(1f))
            // The other half of §3.6, one tap away. What occupies time is
            // here; what is merely owed is there, including everything with no
            // day at all — which this screen has no way to show.
            Text(
                "Outstanding",
                style = MaterialTheme.typography.labelLarge,
                modifier = Modifier.clickable { onTasks() },
            )
        }
        if (entries.isEmpty()) {
            Empty("Nothing on this day.")
            return@Column
        }
        // Three bands, and the first is not a subdivision of the second.
        // An all-day entry is a floating date with no position on an hour
        // axis, and a task that is *due* on a day is a third thing again
        // (§4.4, §3.6).
        val allDay = entries.filter { it.allDay && !it.due }
        val due = entries.filter { it.due }
        val timed = entries.filter { !it.allDay && !it.due }

        LazyColumn {
            if (allDay.isNotEmpty()) {
                item { Band("All day") }
                items(allDay) { Line(it) }
            }
            if (due.isNotEmpty()) {
                item { Band("Due") }
                items(due) { Line(it) }
            }
            if (timed.isNotEmpty()) {
                item { Band("During the day") }
                item { Hours(timed) }
            }
        }
    }
}

/**
 * What is outstanding.
 *
 * Three bands, and the third is the one this screen exists for. A task with no
 * due date reaches no other surface — the timeline carries it in capture order
 * among everything else, and the agenda is a view of time, which §3.6 says is
 * the one thing a task does not occupy.
 *
 * The bands arrive decided. Whether a day has passed is a comparison against
 * today in the reading zone, and this screen working that out for itself would
 * be a second answer to a question the archive has already answered.
 *
 * Finished tasks are not here at all: nothing counts them, and a surface asking
 * what is outstanding is not asking what was.
 */
@Composable
fun Tasks(
    tasks: List<TaskLine>,
    onOpen: (String) -> Unit,
    onDone: (String, Boolean) -> Unit,
    onAgenda: () -> Unit,
) {
    Row(
        Modifier.fillMaxWidth().padding(16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text("Outstanding", style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.weight(1f))
        Text(
            "Agenda",
            style = MaterialTheme.typography.labelLarge,
            modifier = Modifier.clickable { onAgenda() },
        )
    }
    if (tasks.isEmpty()) {
        // Plainly, and as a fact rather than an achievement. §0: nothing here
        // scores the user, so this is not congratulation and not an error.
        Empty("Nothing outstanding.")
        return
    }

    val overdue = tasks.filter { it.band == TaskBand.OVERDUE }
    val due = tasks.filter { it.band == TaskBand.DUE }
    val undated = tasks.filter { it.band == TaskBand.UNDATED }

    LazyColumn(Modifier.fillMaxSize()) {
        if (overdue.isNotEmpty()) {
            item { Band("Overdue") }
            items(overdue) { TaskRow(it, onOpen, onDone) }
        }
        if (due.isNotEmpty()) {
            item { Band("Due") }
            items(due) { TaskRow(it, onOpen, onDone) }
        }
        if (undated.isNotEmpty()) {
            // Labelled even when it is the only band on screen. It is the
            // ordinary case rather than a leftover: the recognizer refuses to
            // promote a bare task verb, so most tasks are named by a person,
            // and naming one is not saying when.
            item { Band("No date") }
            items(undated) { TaskRow(it, onOpen, onDone) }
        }
    }
}

@Composable
private fun TaskRow(task: TaskLine, onOpen: (String) -> Unit, onDone: (String, Boolean) -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clickable { onOpen(task.slug) }
            .padding(16.dp, 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Ticking is the same `setDone` the note page and the command line
        // write. A second path to completion would be a second answer about
        // what done means.
        Checkbox(checked = false, onCheckedChange = { onDone(task.slug, true) })
        Column(Modifier.padding(start = 8.dp)) {
            Text(
                task.text ?: "A capture with no readable text",
                style = MaterialTheme.typography.bodyLarge,
            )
            // The archive's own rendering of the fields, not this screen's.
            if (task.descriptor.isNotEmpty()) {
                Text(task.descriptor, style = MaterialTheme.typography.labelSmall)
            }
        }
    }
}

/**
 * The wiki: named pages, grouped by what they sit under.
 *
 * The parent comes as a field on each page rather than as a tree, so there is
 * nothing here that can get a cycle or an orphan wrong.
 */
@Composable
private fun Band(what: String) {
    Text(
        what,
        Modifier.padding(start = 16.dp, top = 16.dp),
        style = MaterialTheme.typography.titleSmall,
    )
}

@Composable
private fun Line(entry: AgendaEntry) {
    Column(Modifier.fillMaxWidth().padding(16.dp, 6.dp)) {
        if (entry.due) {
            Text(
                if (entry.done) "done" else "not done",
                style = MaterialTheme.typography.labelSmall,
            )
        }
        Text(entry.text ?: entry.slug, style = MaterialTheme.typography.bodyLarge)
    }
    HorizontalDivider()
}

/**
 * The timed part of a day, laid out.
 *
 * Overlapping entries sit side by side, in the columns the archive assigned —
 * §4.4 asks for column packing and the packing is done in Rust, by the same
 * code the desktop runs. Nothing here decides whether two things overlap; it
 * divides the width by a number it was given.
 *
 * A fixed height per hour rather than a scaled one, so an hour is the same
 * distance everywhere on the page. A day drawn to fit would make a busy
 * morning look identical to an empty one.
 */
@Composable
private fun Hours(entries: List<AgendaEntry>) {
    val first = entries.minOf { it.startMs }
    val last = entries.maxOf { it.endMs ?: (it.startMs + UNWRITTEN_MS) }
    val span = (last - first).coerceAtLeast(UNWRITTEN_MS)
    val height = ((span / 60_000).toInt() * MINUTE).coerceAtLeast(120)

    BoxWithConstraints(Modifier.fillMaxWidth().height(height.dp).padding(horizontal = 16.dp)) {
        val full = maxWidth
        // A loop rather than `forEach`: its lambda is not a composable
        // context, so nothing inside it may emit.
        for (entry in entries) {
            val startsAt = ((entry.startMs - first) / 60_000).toInt() * MINUTE
            val runsFor = (((entry.endMs ?: (entry.startMs + UNWRITTEN_MS)) - entry.startMs)
                / 60_000).toInt() * MINUTE
            val width = full / entry.columns.toInt()

            Box(
                Modifier
                    .padding(start = width * entry.column.toInt(), top = startsAt.dp)
                    .width(width)
                    .height(runsFor.coerceAtLeast(24).dp),
            ) {
                Surface(
                    color = MaterialTheme.colorScheme.secondaryContainer,
                    shape = MaterialTheme.shapes.extraSmall,
                    modifier = Modifier.fillMaxWidth().fillMaxHeight().padding(end = 2.dp),
                ) {
                    Text(
                        entry.text ?: entry.slug,
                        Modifier.padding(4.dp),
                        style = MaterialTheme.typography.labelSmall,
                        maxLines = 3,
                    )
                }
            }
        }
    }
}

/**
 * How tall a minute is, and how long an entry with no written end is drawn.
 *
 * The second has to match what the packer reserved. Drawn taller, entries the
 * packer decided were separate would be drawn touching — which is what
 * happened on the desktop, at 13:43 and 14:00, seventeen minutes apart and
 * overlapping on screen.
 */
private const val MINUTE = 1
private const val UNWRITTEN_MS = 15L * 60 * 1000

@Composable
fun Wiki(
    pages: List<Named>,
    people: List<Entity>,
    projects: List<Entity>,
    onOpen: (String) -> Unit,
    onEntity: (String) -> Unit,
) {
    if (pages.isEmpty() && people.isEmpty() && projects.isEmpty()) {
        // Three empty sections rather than one, and it is worth saying which
        // fills itself. A named page is something somebody does; a person or a
        // project appears because a note mentioned one (§3.7).
        Empty(
            "Nothing named yet.\n\n" +
                "A page is a note you gave a name to. People and projects are " +
                "not made at all — write @somebody or ^something in a capture " +
                "and they appear here on their own."
        )
        return
    }
    val known = pages.mapNotNull { it.alias }.toSet()
    LazyColumn(Modifier.fillMaxSize()) {
        // People and projects first, because they populate without anybody
        // doing anything and so are the sections that have something in them.
        if (people.isNotEmpty()) {
            item { Band("People") }
            items(people) { EntityLine(it, "@", onEntity) }
        }
        if (projects.isNotEmpty()) {
            item { Band("Projects") }
            items(projects) { EntityLine(it, "^", onEntity) }
        }
        if (pages.isNotEmpty() && (people.isNotEmpty() || projects.isNotEmpty())) {
            item { Band("Pages") }
        }
        items(pages) { page ->
            Column(
                Modifier.fillMaxWidth().clickable { onOpen(page.alias ?: page.slug) }
                    .padding(start = if (page.parent != null && known.contains(page.parent)) 32.dp else 16.dp,
                             end = 16.dp, top = 8.dp, bottom = 8.dp)
            ) {
                Text(page.alias ?: page.slug, style = MaterialTheme.typography.bodyLarge)
                if (page.parent != null) {
                    Text("under ${page.parent}", style = MaterialTheme.typography.labelSmall)
                }
            }
            HorizontalDivider()
        }
    }
}

/**
 * A note's text, with its mentions leading to their pages.
 *
 * The runs come from the archive. What counts as a mention is a rule the store
 * owns, and a second implementation of it here would disagree about an email
 * address today and about whatever the rule becomes tomorrow.
 *
 * Built as one `AnnotatedString` rather than a row of separate `Text`s so that
 * the paragraph wraps as a paragraph — pieces laid side by side break lines at
 * the mentions instead of at the words.
 */
@Composable
private fun MentionedText(text: String, model: Model) {
    val pieces = remember(text) { model.pieces(text) }
    if (pieces.none { it.mention != null }) {
        Text(text, style = MaterialTheme.typography.bodyLarge)
        return
    }

    // Offsets are accumulated as the string is built, which is arithmetic rather
    // than a decision: the archive said which runs are mentions and this only
    // records where each one landed.
    // Read outside `remember`: the theme is a composable lookup and cannot be
    // done inside one.
    val accent = MaterialTheme.colorScheme.primary
    val spans = remember(text) { mutableListOf<Triple<Int, Int, String>>() }
    val annotated = remember(text) {
        spans.clear()
        buildAnnotatedString {
            for (piece in pieces) {
                val at = length
                val mention = piece.mention
                if (mention != null) {
                    withStyle(
                        SpanStyle(color = accent, textDecoration = TextDecoration.Underline)
                    ) { append(piece.text) }
                    spans.add(Triple(at, length, mention))
                } else {
                    append(piece.text)
                }
            }
        }
    }

    ClickableText(
        annotated,
        style = MaterialTheme.typography.bodyLarge.copy(
            color = MaterialTheme.colorScheme.onSurface,
        ),
    ) { offset ->
        spans.firstOrNull { offset >= it.first && offset < it.second }?.let {
            model.go(Screen.Entity(it.third))
        }
    }
}

@Composable
private fun EntityLine(entity: Entity, sigil: String, onOpen: (String) -> Unit) {
    // Opened by the mention as written, not by the display name: a renamed
    // entity is still reached by what the notes actually say.
    val mention = sigil + (entity.names.firstOrNull() ?: entity.display)
    Column(
        Modifier
            .fillMaxWidth()
            .clickable { onOpen(mention) }
            .padding(16.dp, 8.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(entity.display, style = MaterialTheme.typography.bodyLarge)
            Spacer(Modifier.weight(1f))
            Text(
                "${entity.notes.size}",
                style = MaterialTheme.typography.labelMedium,
            )
        }
        // What else reaches it, after a merge. Shown because a page that quietly
        // answers to three names is one somebody will think has lost notes.
        if (entity.names.size > 1) {
            Text(
                "also ${entity.names.joinToString(", ")}",
                style = MaterialTheme.typography.labelSmall,
            )
        }
    }
    HorizontalDivider()
}

/**
 * A person, a project or a tag: what backs it, and what names it.
 *
 * The backing note sits above the list and is absent from it — §4.3's "the note
 * that *is* the project, rather than one that mentions it". A row appearing in
 * both places reads as two notes.
 *
 * The page is handed over whole by the archive. Nothing here composes it from a
 * query and a count, because two surfaces assembling a page separately is two
 * answers to one question.
 */
@Composable
fun EntityPage(
    mention: String,
    entity: Entity?,
    onOpen: (String) -> Unit,
    model: Model,
) {
    if (entity == null) {
        // Not an error and not a missing record. An entity exists because a note
        // names one, so there is nothing here to have failed.
        Empty(
            "Nothing names $mention.\n\n" +
                "Pages are not created — they exist for whatever your notes say. " +
                "Write $mention in a capture and this one fills itself in."
        )
        return
    }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(entity.display, style = MaterialTheme.typography.titleMedium)
            Text(
                "${entity.kind} · ${entity.notes.size} note(s)",
                style = MaterialTheme.typography.labelSmall,
            )
            if (entity.names.size > 1) {
                Text(
                    "also ${entity.names.joinToString(", ")}",
                    style = MaterialTheme.typography.labelSmall,
                )
            }
        }

        entity.backing?.let { note ->
            Text(
                "the note that is this ${entity.kind}",
                Modifier.padding(start = 16.dp, top = 8.dp),
                style = MaterialTheme.typography.labelSmall,
            )
            // Not tickable from here. A backing note's list items belong on its
            // own page; a tick on this one would be a gesture nobody aimed at.
            NoteRow(note, onOpen = onOpen, onTick = { _, _, _ -> })
            HorizontalDivider()
        }

        EntityActions(mention, entity, model)
        HorizontalDivider()

        if (entity.notes.isEmpty()) {
            Empty("Nothing else names it now.")
            return@Column
        }
        for (note in entity.notes) {
            NoteRow(note, onOpen = onOpen, onTick = model::tickAt)
        }
    }
}

/**
 * Renaming, merging and binding, where the page is.
 *
 * §8.4's gradient: a correction that needs a different machine than the one that
 * found the problem is the wrong way round, and the phone is where a duplicated
 * person is first noticed because the phone is where captures are made.
 *
 * Merging asks, because it is the one operation here that makes two pages one.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun EntityActions(mention: String, entity: Entity, model: Model) {
    var asking by remember { mutableStateOf<String?>(null) }
    var answer by remember { mutableStateOf("") }

    FlowRow(
        Modifier.fillMaxWidth().padding(16.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        TextButton({
            answer = entity.display
            asking = "rename"
        }) { Text("Rename") }
        TextButton({
            answer = ""
            asking = "merge"
        }) { Text("Merge into") }
        if (entity.backing == null) {
            TextButton({
                answer = ""
                asking = "bind"
            }) { Text("Backing note") }
        } else {
            TextButton({ model.bindBacking(mention, null) }) { Text("Unbind") }
        }
    }

    asking?.let { what ->
        val (title, hint) = when (what) {
            "rename" -> "Rename" to "What to call it. The notes are unchanged."
            "merge" -> "Merge into" to
                "The other one, as written — @sarah-chen. Nothing moves: both " +
                    "names reach one page and every note still says what it said."
            else -> "Backing note" to "The note that *is* this ${entity.kind}, by name."
        }
        AlertDialog(
            onDismissRequest = { asking = null },
            title = { Text(title) },
            text = {
                Column {
                    Text(hint, style = MaterialTheme.typography.labelSmall)
                    TextField(answer, { answer = it }, Modifier.fillMaxWidth())
                }
            },
            confirmButton = {
                Button({
                    when (what) {
                        "rename" -> model.renameEntity(mention, answer)
                        // `from` is the other one and `into` is this page, so the
                        // surface somebody is looking at is the one that survives.
                        "merge" -> model.mergeEntities(answer, mention)
                        else -> model.bindBacking(mention, answer.ifBlank { null })
                    }
                    asking = null
                }) { Text(title) }
            },
            dismissButton = { TextButton({ asking = null }) { Text("Cancel") } },
        )
    }
}

/**
 * A list: what is on it, and what has been bought.
 *
 * The surface this whole change exists for. §2.1's argument about capture
 * applies again at the other end — standing in a shop, one hand on the phone,
 * the gesture has to be a thumb landing on a row and nothing else. So the whole
 * row is the target rather than a checkbox somebody has to hit.
 *
 * Done items stay, struck through rather than removed. A row that vanished
 * would leave nothing to tap when it turns out the shop had none, and §6 rates
 * a state that cannot be reversed from any device as the failure worth
 * avoiding. Whether they should eventually hide behind a toggle is the open
 * question this change deliberately left for a week of real use to answer.
 */
@Composable
fun ListScreen(
    tag: String,
    lines: List<ListLine>,
    onTick: (String, UInt, Boolean) -> Unit,
) {
    if (lines.isEmpty()) {
        Empty("Nothing on the #$tag list. Capture one: #$tag: first thing, second thing")
        return
    }
    // How many captures wrote what is on screen.
    //
    // Said out loud when it is more than one, because this is a *collection* —
    // everything carrying the tag — and not one person's single list. Drawing
    // two shopping trips as one list is how a tag came to be treated as a
    // list's identity, and somebody reading it has no other way to tell.
    val sources = lines.map { it.slug }.filter { it.isNotEmpty() }.distinct()
    LazyColumn(Modifier.fillMaxSize()) {
        if (sources.size > 1) {
            item {
                Text(
                    "Everything tagged #$tag, from ${sources.size} captures.",
                    style = MaterialTheme.typography.labelSmall,
                    modifier = Modifier.padding(16.dp, 8.dp),
                )
            }
        }
        items(lines) { line ->
            Row(
                Modifier.fillMaxWidth()
                    // By position, which is what the row knows. Two items
                    // reading the same were untickable from here until this,
                    // because matching by words found both and refused.
                    .clickable {
                        line.ord?.let { onTick(line.slug, it, !line.done) }
                    }
                    .padding(horizontal = 16.dp, vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Checkbox(
                    checked = line.done,
                    // Null, because the row already handles it. A checkbox with
                    // its own handler makes two targets out of one gesture and
                    // the smaller one is the one a thumb misses.
                    onCheckedChange = null,
                )
                Text(
                    line.text,
                    style = MaterialTheme.typography.bodyLarge,
                    textDecoration = if (line.done) TextDecoration.LineThrough else null,
                    modifier = Modifier.padding(start = 12.dp),
                )
            }
            HorizontalDivider()
        }
        item {
            val left = lines.count { !it.done }
            Text(
                if (left == 0) "All ${lines.size} done." else "$left of ${lines.size} left.",
                style = MaterialTheme.typography.labelMedium,
                modifier = Modifier.padding(16.dp),
            )
        }
    }
}

/**
 * What the archive would like someone to know.
 *
 * Says so when there is nothing, rather than showing an empty box. An empty
 * surface is ambiguous between "nothing is wrong" and "nothing was checked",
 * and that ambiguity is a cost the system would be manufacturing for free.
 */
@Composable
fun Health(
    conditions: List<Condition>,
    peers: List<Peer>,
    costs: List<String>,
    notifier: String?,
    me: String,
    onNotifier: (String?) -> Unit,
    onSync: () -> Unit,
    onPair: () -> Unit,
) {
    val attention = conditions.filter { it.tier == "attention" }
    val knowing = conditions.filter { it.tier != "attention" }

    LazyColumn(Modifier.fillMaxSize().padding(16.dp)) {
        item { Text("Needs attention", style = MaterialTheme.typography.titleSmall) }
        if (attention.isEmpty()) {
            item { Text("Nothing needs your attention.", Modifier.padding(vertical = 8.dp)) }
        }
        items(attention) { Card(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
            Text(it.text, Modifier.padding(12.dp))
        } }

        if (knowing.isNotEmpty()) {
            item { Text("Worth knowing", style = MaterialTheme.typography.titleSmall,
                        modifier = Modifier.padding(top = 16.dp)) }
            items(knowing) { Card(Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
                Text(it.text, Modifier.padding(12.dp))
            } }
        }

        item {
            Text(
                "Devices",
                style = MaterialTheme.typography.titleSmall,
                modifier = Modifier.padding(top = 24.dp),
            )
        }
        if (peers.isEmpty()) {
            item {
                Text(
                    "No other devices yet. Pair one to sync.",
                    Modifier.padding(vertical = 8.dp),
                )
            }
        }
        item {
            Column(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
                Text("$me (this phone)", style = MaterialTheme.typography.bodyMedium)
                Text(
                    if (notifier == me) "Says things when a cue fires" else "",
                    style = MaterialTheme.typography.labelSmall,
                )
                if (notifier != me) {
                    TextButton({ onNotifier(me) }) { Text("Notify me here") }
                }
            }
        }
        items(peers) { peer ->
            Column(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
                Text(peer.key, style = MaterialTheme.typography.bodyMedium)
                Text(
                    // What it can watch for, so a notifier is chosen among
                    // devices that can deliver (§7.5). "Not known yet" rather
                    // than an empty list read as a capability: a device that
                    // has not been synced with has said nothing, which is not
                    // the same as saying it watches nothing.
                    if (peer.observes.isEmpty()) {
                        "Has not said what it can watch for"
                    } else {
                        "Watches for ${peer.observes.joinToString(", ")}"
                    },
                    style = MaterialTheme.typography.labelSmall,
                )
                if (notifier == peer.key) {
                    Text(
                        "Says things when a cue fires",
                        style = MaterialTheme.typography.labelSmall,
                    )
                } else {
                    TextButton({ onNotifier(peer.key) }) { Text("Notify me there") }
                }
                // When, never "fresh" or "stale" (§6.6). The device that
                // sleeps most is the one that must not imply currency, and
                // this is that device.
                Text(
                    peer.lastSeen?.let { "Last reached $it" } ?: "Never reached from here",
                    style = MaterialTheme.typography.labelSmall,
                )
                if (peer.missed > 0u) {
                    Text(
                        "${peer.missed} attempt(s) since",
                        style = MaterialTheme.typography.labelSmall,
                    )
                }
            }
        }
        item {
            // Said out loud, because the difference is the whole of §8.9: a
            // value nobody chose is a rule that a better rule can replace, and
            // a value somebody chose is theirs. Writing the first down as the
            // second destroys the distinction permanently.
            Text(
                notifier?.let { "Chosen by you." }
                    ?: "Nobody has chosen. Whichever device sees a cue says so.",
                style = MaterialTheme.typography.labelSmall,
                modifier = Modifier.padding(top = 8.dp),
            )
            if (notifier != null) {
                TextButton({ onNotifier(null) }) { Text("Stop choosing") }
            }
            Row {
                TextButton(onSync) { Text("Sync now") }
                TextButton(onPair) { Text("Pair a device") }
            }
        }

        if (costs.isNotEmpty()) {
            item {
                Text(
                    "What capturing costs",
                    style = MaterialTheme.typography.titleSmall,
                    modifier = Modifier.padding(top = 24.dp),
                )
                Text(
                    // Measured, not claimed. §2.1's argument is that capture
                    // has to cost almost nothing at the moment a thought
                    // arrives, and this is the only thing that can say whether
                    // it does on this device.
                    "Commit to stored, and how long each window took to appear. " +
                        "The time spent typing or speaking is the person's, not " +
                        "the system's, and measuring it would report a considered " +
                        "sentence as a slow surface.",
                    style = MaterialTheme.typography.labelSmall,
                )
            }
            items(costs) {
                Text(it, Modifier.padding(vertical = 4.dp), style = MaterialTheme.typography.labelMedium)
            }
        }

        item {
            Text(
                "Signatures, body hashes and origin chains are not checked here.",
                style = MaterialTheme.typography.labelSmall,
                modifier = Modifier.padding(top = 24.dp),
            )
        }
    }
}

@Composable
fun Empty(what: String) {
    Column(Modifier.fillMaxSize().padding(24.dp)) {
        Text(what, style = MaterialTheme.typography.bodyMedium)
    }
}
