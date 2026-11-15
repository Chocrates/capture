package dev.capture

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.VerticalDivider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import uniffi.capture_ffi.AgendaEntry
import uniffi.capture_ffi.CalendarShape
import uniffi.capture_ffi.CalendarView
import uniffi.capture_ffi.GridDay

/**
 * The calendar: a month grid or a week strip, and the chosen day beneath.
 *
 * # Why it is shaped like this
 *
 * Found in use on 2026-10-12: the agenda showed today and could not leave it,
 * so "what does next week look like" and "when is the dentist" had no answer
 * short of searching. The person asked for both a month grid and a week
 * strip; they share the chosen day, so switching keeps your place.
 *
 * Everything here arrives drawn. Which weekday the 1st falls on, how long the
 * month is, where the arrows go, what "14:00–15:00" reads as — all of that is
 * date arithmetic, which this build refuses in Kotlin, and the grid's dots and
 * the list beneath come from the same read so they cannot disagree.
 */
@Composable
fun Agenda(day: String, model: Model, onTasks: () -> Unit, onOpen: (String) -> Unit) {
    val view = model.calendar
    // The one occurrence whose sheet is open. Held here so the sheet survives
    // the list being re-read underneath it.
    var picked by remember { mutableStateOf<AgendaEntry?>(null) }

    val list = @Composable {
        DayList(
            label = view?.days?.firstOrNull { it.date == day }?.label ?: day,
            entries = model.agenda,
            onPick = { picked = it },
            onHour = model::settleHourHere,
            onDay = model::settleDayHere,
        )
    }
    // Held sideways, the grid beside the day rather than above it. Stacked, a
    // landscape phone gave the grid the whole height and the day's list none:
    // seen on 2026-10-13, the chosen day's entries were not on screen at all.
    BoxWithConstraints(Modifier.fillMaxSize()) {
        if (maxWidth > maxHeight) {
            Row(Modifier.fillMaxSize()) {
                Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
                    Header(view, model.shape, day, onTasks, model)
                    if (view != null) Grid(view, model.shape, day, model::showDay)
                }
                VerticalDivider()
                Column(Modifier.weight(1f)) { list() }
            }
        } else {
            Column(Modifier.fillMaxSize()) {
                Header(view, model.shape, day, onTasks, model)
                if (view != null) Grid(view, model.shape, day, model::showDay)
                HorizontalDivider()
                list()
            }
        }
    }

    picked?.let { entry ->
        Occurrence(entry, day, model, onOpen) { picked = null }
    }
}

@Composable
private fun Header(
    view: CalendarView?,
    shape: CalendarShape,
    day: String,
    onTasks: () -> Unit,
    model: Model,
) {
    Column(Modifier.fillMaxWidth().padding(horizontal = 8.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            TextButton({ view?.let { model.showDay(it.previous) } }) { Text("‹") }
            Text(
                view?.title ?: day,
                style = MaterialTheme.typography.titleMedium,
                modifier = Modifier.weight(1f),
            )
            TextButton({ view?.let { model.showDay(it.next) } }) { Text("›") }
        }
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            FilterChip(
                selected = shape == CalendarShape.MONTH,
                onClick = { model.drawAs(CalendarShape.MONTH) },
                label = { Text("Month") },
            )
            FilterChip(
                selected = shape == CalendarShape.WEEK,
                onClick = { model.drawAs(CalendarShape.WEEK) },
                label = { Text("Week") },
            )
            Spacer(Modifier.weight(1f))
            if (view != null && view.today != day) {
                TextButton({ model.showDay(view.today) }) { Text("Today") }
            }
            // The other half of §3.6, one tap away: what is merely owed,
            // including everything with no day at all.
            TextButton(onTasks) { Text("Outstanding") }
        }
    }
}

/**
 * The cells. Swiping moves a month or a week, as the arrows do.
 *
 * A cell marks a busy day with a dot rather than a title: a phone cell is too
 * small for words, and a truncated title reads as a different appointment.
 */
@Composable
private fun Grid(view: CalendarView, shape: CalendarShape, day: String, onDay: (String) -> Unit) {
    var dragged by remember { mutableStateOf(0f) }
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = 8.dp)
            .pointerInput(view.previous, view.next) {
                detectHorizontalDragGestures(
                    onDragStart = { dragged = 0f },
                    onDragEnd = {
                        when {
                            dragged > 120f -> onDay(view.previous)
                            dragged < -120f -> onDay(view.next)
                        }
                    },
                    onHorizontalDrag = { _, by -> dragged += by },
                )
            },
    ) {
        Row(Modifier.fillMaxWidth()) {
            for (name in listOf("M", "T", "W", "T", "F", "S", "S")) {
                Text(
                    name,
                    Modifier.weight(1f).padding(vertical = 4.dp),
                    style = MaterialTheme.typography.labelSmall,
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                )
            }
        }
        for (week in view.days.chunked(7)) {
            Row(Modifier.fillMaxWidth()) {
                for (cell in week) {
                    Cell(cell, cell.date == day, shape, Modifier.weight(1f)) { onDay(cell.date) }
                }
            }
        }
    }
}

@Composable
private fun Cell(
    cell: GridDay,
    chosen: Boolean,
    shape: CalendarShape,
    modifier: Modifier,
    onClick: () -> Unit,
) {
    val colors = MaterialTheme.colorScheme
    Box(
        modifier
            // A height, not an aspect ratio: a ratio grows with the width, and
            // a phone held sideways drew cells a third of the screen tall.
            .height(if (shape == CalendarShape.WEEK) 64.dp else 48.dp)
            .padding(2.dp)
            .then(
                if (chosen) {
                    Modifier.background(colors.primaryContainer, CircleShape)
                } else if (cell.today) {
                    Modifier.border(1.dp, colors.primary, CircleShape)
                } else {
                    Modifier
                },
            )
            .clickable(onClick = onClick)
            .alpha(if (cell.inPeriod) 1f else 0.4f),
        contentAlignment = Alignment.Center,
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text("${cell.dayOfMonth}", style = MaterialTheme.typography.bodyMedium)
            Box(
                Modifier
                    .padding(top = 2.dp)
                    .size(5.dp)
                    .background(if (cell.busy) colors.primary else Color.Transparent, CircleShape),
            )
        }
    }
}

/**
 * The chosen day: all day, due, during the day, and what was skipped.
 *
 * Skipped occurrences are listed rather than hidden (§4.4: "hidden but
 * recoverable") — recoverable needs somewhere to press Restore. They are not
 * on the hour axis, where they would push something real into a narrower
 * column.
 */
@Composable
private fun DayList(
    label: String,
    entries: List<AgendaEntry>,
    onPick: (AgendaEntry) -> Unit,
    onHour: (String, Boolean) -> Unit,
    onDay: (String, UInt) -> Unit,
) {
    val allDay = entries.filter { it.allDay && !it.due && !it.skipped }
    val due = entries.filter { it.due }
    val timed = entries.filter { !it.allDay && !it.due && !it.skipped }
    val skipped = entries.filter { it.skipped }

    LazyColumn(Modifier.fillMaxSize()) {
        item {
            Text(
                label,
                Modifier.padding(start = 16.dp, top = 12.dp),
                style = MaterialTheme.typography.titleSmall,
            )
        }
        if (entries.isEmpty()) {
            item { Empty("Nothing on this day.") }
        }
        if (allDay.isNotEmpty()) {
            item { Band("All day") }
            items(allDay) { EntryRow(it, onPick, onHour, onDay) }
        }
        if (due.isNotEmpty()) {
            item { Band("Due") }
            items(due) { EntryRow(it, onPick, onHour, onDay) }
        }
        if (timed.isNotEmpty()) {
            item { Band("During the day") }
            // Drawn once, on the hour axis, where overlaps sit side by side
            // (§4.4). Each block is tappable, as a row would be.
            item { Hours(timed, onPick) }
        }
        if (skipped.isNotEmpty()) {
            item { Band("Skipped") }
            items(skipped) { EntryRow(it, onPick, onHour, onDay) }
        }
    }
}

@Composable
private fun EntryRow(
    entry: AgendaEntry,
    onPick: (AgendaEntry) -> Unit,
    onHour: (String, Boolean) -> Unit,
    onDay: (String, UInt) -> Unit,
) {
    Column(
        Modifier
            .fillMaxWidth()
            .clickable { onPick(entry) }
            .padding(16.dp, 8.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                entry.at,
                Modifier.padding(end = 12.dp),
                style = MaterialTheme.typography.labelMedium,
            )
            Text(
                entry.title ?: entry.text ?: entry.slug,
                style = MaterialTheme.typography.bodyLarge,
                textDecoration = if (entry.skipped || entry.done) TextDecoration.LineThrough else null,
            )
        }
        entry.movedLabel?.let { Text(it, style = MaterialTheme.typography.labelSmall) }
        // What a sun time hangs from, and whether the phone knows where it is.
        entry.sun?.let { Text(it, style = MaterialTheme.typography.labelSmall) }
        if (entry.done) Text("done", style = MaterialTheme.typography.labelSmall)
        // Why this has no time, asked where it is seen. Each button says what
        // it will write, because the answer goes into the text.
        entry.unclearHour?.let { h ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    "No time yet — \"at $h\" could be either.",
                    style = MaterialTheme.typography.labelSmall,
                )
                TextButton({ onHour(entry.slug, false) }) { Text("${h}am") }
                TextButton({ onHour(entry.slug, true) }) { Text("${h}pm") }
            }
        }
        // On both days it could mean, and on neither until answered: "next
        // thursday" said on a Tuesday is this week's to some people and next
        // week's to others. Each button names the day it will write.
        if (entry.unclearDay.size == 2) {
            // The row's time column already says "which day?".
            Row(verticalAlignment = Alignment.CenterVertically) {
                entry.unclearDay.forEachIndexed { i, label ->
                    TextButton({ onDay(entry.slug, i.toUInt()) }) { Text(label) }
                }
            }
        }
    }
    HorizontalDivider()
}

/**
 * What can be done to one thing on the calendar.
 *
 * For an occurrence of a series: done, move this one, skip this one, change
 * this and later (§3.5's three scopes, minus "all", which is editing the
 * note). Move takes words — "thursday 10am" — and shows where they land
 * before anything is written, so a misread day is seen and not lived.
 */
@Composable
private fun Occurrence(
    entry: AgendaEntry,
    day: String,
    model: Model,
    onOpen: (String) -> Unit,
    onClose: () -> Unit,
) {
    val occ = entry.occ
    // Which form is open: none, moving, or changing this and later.
    var mode by remember { mutableStateOf("") }
    var text by remember { mutableStateOf("") }
    var landing by remember { mutableStateOf<Result<String>?>(null) }

    LaunchedEffect(mode, text) {
        landing = null
        if (mode == "move" && occ != null && text.isNotBlank()) {
            delay(250)
            landing = model.previewMove(entry.slug, occ, text)
        }
    }

    AlertDialog(
        onDismissRequest = onClose,
        title = { Text(entry.title ?: entry.text ?: entry.slug) },
        text = {
            Column {
                Text(entry.at, style = MaterialTheme.typography.labelMedium)
                entry.movedLabel?.let { Text(it, style = MaterialTheme.typography.labelSmall) }
                entry.sun?.let { Text(it, style = MaterialTheme.typography.labelSmall) }
                when (mode) {
                    "move" -> {
                        OutlinedTextField(
                            value = text,
                            onValueChange = { text = it },
                            label = { Text("When instead — e.g. thursday 10am") },
                            singleLine = true,
                            modifier = Modifier.padding(top = 8.dp),
                        )
                        landing?.let { r ->
                            Text(
                                r.fold({ "→ $it" }, { it.message ?: "Could not read that." }),
                                style = MaterialTheme.typography.bodySmall,
                                modifier = Modifier.padding(top = 4.dp),
                            )
                        }
                    }
                    "later" -> {
                        Text(
                            "From $day on, this repeats as:",
                            style = MaterialTheme.typography.bodySmall,
                            modifier = Modifier.padding(top = 8.dp),
                        )
                        OutlinedTextField(
                            value = text,
                            onValueChange = { text = it },
                            label = { Text("e.g. every tuesday at 6pm") },
                            singleLine = true,
                        )
                        Text(
                            "Earlier ones keep the old rule.",
                            style = MaterialTheme.typography.labelSmall,
                        )
                    }
                    else -> Column {
                        if (occ != null) {
                            TextButton({
                                model.setOccurrenceDone(entry.slug, occ, !entry.done)
                                onClose()
                            }) { Text(if (entry.done) "Not done" else "Done") }
                            TextButton({ mode = "move"; text = "" }) { Text("Move this one") }
                            if (entry.skipped || entry.movedFrom != null) {
                                // Back where the rule put it. For a skipped one
                                // that is Restore; for a moved one, Move back.
                                TextButton({
                                    model.restoreOccurrence(entry.slug, occ)
                                    onClose()
                                }) { Text(if (entry.skipped) "Restore" else "Move back") }
                            }
                            if (!entry.skipped) {
                                TextButton({
                                    model.skipOccurrence(entry.slug, occ)
                                    onClose()
                                }) { Text("Skip this one") }
                            }
                            if (entry.series) {
                                TextButton({ mode = "later"; text = "" }) {
                                    Text("Change this and later")
                                }
                            }
                        }
                        TextButton({
                            onClose()
                            onOpen(entry.slug)
                        }) { Text("Open the note") }
                    }
                }
            }
        },
        confirmButton = {
            when (mode) {
                "move" -> TextButton(
                    onClick = {
                        if (occ != null) model.moveOccurrence(entry.slug, occ, text)
                        onClose()
                    },
                    enabled = landing?.isSuccess == true,
                ) { Text("Move") }
                "later" -> TextButton(
                    onClick = {
                        model.splitSeries(entry.slug, day, text)
                        onClose()
                    },
                    enabled = text.isNotBlank(),
                ) { Text("Change") }
                else -> {}
            }
        },
        dismissButton = { TextButton(onClose) { Text("Cancel") } },
    )
}
