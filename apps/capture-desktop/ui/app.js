// The browse surface.
//
// Every call into the store goes through `invoke`, which reaches exactly the
// commands enumerated in ipc.rs and nothing else. There is no fetch here, no
// URL, and no server to talk to.
//
// Note text is inserted with textContent, never innerHTML. Captures are stored
// verbatim and are expected to contain anything at all — angle brackets,
// ampersands, half-written HTML — and treating any of it as markup would let a
// note change the page it is displayed on.

// `window.__TAURI__` exists only because `withGlobalTauri` is set in
// tauri.conf.json. It defaults to false, and with it unset this line throws
// before anything is rendered — the whole script aborts on its first statement
// and the window comes up empty with nothing in the log to say why. Said here
// because the symptom points at the store, the webview, or the build, and the
// cause is one boolean in a file this script never mentions.
if (!window.__TAURI__) {
  document.getElementById("view").textContent =
    "The Tauri API was not injected — check withGlobalTauri in tauri.conf.json.";
  throw new Error("window.__TAURI__ is missing; withGlobalTauri is not set");
}

const invoke = window.__TAURI__.core.invoke;
const view = document.getElementById("view");

// Say so on the page when something throws.
//
// Written because of how long this took to find. `showTimeline` awaits
// `renderPending` before it asks for any notes, so a throw in the pending
// redaction block left the whole window empty — no notes, no error, nothing in
// the process log, and a store that was perfectly fine. An empty view is
// indistinguishable from an empty store, and the empty state below says
// "Nothing captured yet", which was actively the wrong story.
//
// This is diagnosis, not error handling. Anything it catches is a bug here
// rather than a condition to recover from, so it says what happened and stops.
function fatal(what) {
  const v = document.getElementById("view");
  if (!v) return;
  v.replaceChildren();
  v.append(el("h2", null, "Something in this window is broken"));
  v.append(el("p", null, String(what)));
  v.append(el("p", "hint", "Your notes are unaffected — this is the view, not the store."));
}

window.addEventListener("error", (e) => fatal(e.message || e.error || e));
window.addEventListener("unhandledrejection", (e) => {
  // A store error is not a broken page, and the two need different words. The
  // poll will try again; `fatal` would stop and blame the window.
  if (e.reason instanceof StoreError) {
    notReady(e.reason);
    return;
  }
  fatal((e.reason && (e.reason.message || e.reason)) || "a promise was rejected");
});

// Which view is on screen.
//
// Load-bearing beyond the nav highlight: `refresh` reads it to decide whether
// redrawing is safe. Every function that replaces the view sets it, and one
// that forgets to would have the timeline reappear underneath whatever it drew.
let current = "timeline";
let agendaDay = null;
let currentNote = null;
let currentMention = null;

// Where you have been, and where you were before you went back.
//
// §4.3 asks for this outright — "navigation keeps a back/forward history, since
// click-through is the entire point" — and it is the difference between a wiki
// and a set of pages. Following a link three notes deep and having no way back
// except the timeline makes the third link one nobody clicks.
//
// Two stacks, which is how every browser does it and for the same reason: going
// back does not erase where you were, and going somewhere new does. Entries are
// the arguments to a view, not rendered pages — a page re-read on the way back
// shows what the note says now, and a snapshot would show what it said then.
const history = { back: [], forward: [] };

/// Where we are, as something that can be gone back to.
///
/// Any browsable view, not only notes. The first version recorded notes alone,
/// which broke the path the entity pages exist to create: timeline → `@sarah` →
/// one of her notes left no way back to `@sarah`, because the page in the
/// middle was never written down.
function here() {
  if (current === "wiki") return { view: "wiki", at: wikiAt };
  if (current === "lists" && listTag) return { view: "lists", tag: listTag };
  if (current === "note" && currentNote) return { view: "note", name: currentNote };
  if (current === "mentions" && currentMention) return { view: "mentions", ...currentMention };
  return null;
}

function same(a, b) {
  return a && b && JSON.stringify(a) === JSON.stringify(b);
}

/// Go to a recorded place.
function go(entry) {
  if (entry.view === "wiki") return showWiki(entry.at);
  if (entry.view === "lists") return showLists(entry.tag);
  if (entry.view === "note") return showNote(entry.name);
  if (entry.view === "mentions") return showMentions(entry.sigil, entry.value);
}

/// Record where we are before going somewhere else.
///
/// Called by navigation, never by a redraw. `showNote` refreshing itself after
/// an edit must not push a second copy of the same note, or "back" walks
/// through the same page once per change made to it.
function visit(entry) {
  const from = here();
  if (from && !same(from, entry)) history.back.push(from);
  // A new destination ends the forward branch, exactly as it does in a
  // browser: the path you came back from is no longer the path you are on.
  history.forward.length = 0;
  drawHistory();
}

/// The two buttons, in the navbar rather than on a page.
///
/// They were on the note page, which is the one place they are least needed —
/// a note already offers its links, its backlinks and the timeline. Somewhere
/// three clicks deep is exactly where you cannot see them, and a control that
/// appears and disappears as you move is one nobody learns.
///
/// Disabled rather than hidden, for the same reason: a button that vanishes
/// takes its meaning with it, and the layout moves under the cursor.
function drawHistory() {
  const b = document.getElementById("back");
  const f = document.getElementById("forward");
  if (!b || !f) return;
  b.disabled = history.back.length === 0;
  f.disabled = history.forward.length === 0;
}

// The session on offer, held rather than re-fetched.
//
// Asking for a session is a write: being offered a note is the outcome `shown`,
// which moves `last_shown` and makes those notes ineligible for a day. Running
// `capture review` twice in a terminal is two deliberate acts and spending the
// session is fair. Clicking a nav button twice is not, and a pane that quietly
// consumed somebody's five notes because they looked at the timeline in between
// would be punishing them for navigating.
//
// So the session is fetched once and kept. It is over when it is over, and the
// next one comes with the next window.
let session = null;

// Which render is the current one.
//
// Every view clears the page, then awaits the store, then appends. Two of those
// running at once — a click and the refresh poll landing in the same moment —
// both clear and then both append, and the page shows everything twice. It is
// not a data bug and looking at the store will never explain it.
//
// So each render takes a number, and after every await it checks that it is
// still the newest. A render that has been overtaken stops rather than drawing
// on top of its successor.
let renderSeq = 0;

function beginRender() {
  renderSeq += 1;
  return renderSeq;
}

function stale(mine) {
  return mine !== renderSeq;
}

// ----------------------------------------------------------------- agenda ---

/// What occurs and what is due, for one day.
///
/// Two bands, and that is a consequence of the time model rather than a layout
/// choice (§4.4): all-day entries are floating dates and have no position on an
/// hour axis, so putting them in the timed list would imply one.
async function showAgenda(day) {
  const mine = beginRender();
  current = "agenda";
  agendaDay = day || today();
  view.replaceChildren();

  const bar = el("div", "row");
  const back = el("button", "act", "← timeline");
  back.addEventListener("click", showTimeline);
  bar.append(back);

  const prev = el("button", "act", "◀");
  prev.addEventListener("click", () => showAgenda(shiftDay(agendaDay, -1)));
  const next = el("button", "act", "▶");
  next.addEventListener("click", () => showAgenda(shiftDay(agendaDay, 1)));
  bar.append(prev, next);
  if (agendaDay !== today()) {
    const back = el("button", "act", "today");
    back.addEventListener("click", () => showAgenda(today()));
    bar.append(back);
  }
  view.append(bar);

  // The month beside the day: which days have something, one click to any of
  // them. Drawn from the same read as the day, so a dot and the list under it
  // cannot disagree.
  const [month, rows] = await Promise.all([
    call("month", { day: agendaDay }),
    call("agenda", { day: agendaDay }),
  ]);
  if (stale(mine)) return;
  view.append(monthGrid(month));

  view.append(el("h2", null, agendaDay));

  if (!rows.length) {
    view.append(el("p", "empty", "Nothing on this day."));
    return;
  }

  // Three bands, and the first is not a subdivision of the second. "All day"
  // is a claim about an event's extent — it occupies the whole of the day — and
  // a task due today occupies none of it. Filing one under the other said
  // something false about every task in the list, and said it most loudly about
  // an overdue one, which is not on this day at all.
  const due = rows.filter((r) => r.due);
  const allDay = rows.filter((r) => !r.due && r.all_day && !r.skipped);
  const timed = rows.filter((r) => !r.due && !r.all_day && !r.skipped);
  // Hidden from the day, not gone (§4.4): listed, so Restore has a place.
  const skipped = rows.filter((r) => r.skipped);

  if (due.length) {
    const band = el("div", "all-day");
    band.append(el("div", "band-label", "due"));
    for (const r of due) band.append(agendaRow(r));
    view.append(band);
  }
  if (allDay.length) {
    const band = el("div", "all-day");
    band.append(el("div", "band-label", "all day"));
    for (const r of allDay) band.append(agendaRow(r));
    view.append(band);
  }
  view.append(grid(timed));
  if (timed.length) {
    // The grid's blocks tick; the rows here carry the rest of what can be
    // done to one occurrence.
    const band = el("div", "all-day");
    band.append(el("div", "band-label", "during the day"));
    for (const r of timed) band.append(agendaRow(r));
    view.append(band);
  }
  if (skipped.length) {
    const band = el("div", "all-day");
    band.append(el("div", "band-label", "skipped"));
    for (const r of skipped) band.append(agendaRow(r));
    view.append(band);
  }
}

/// A month as whole weeks, busy days dotted, the chosen day ringed.
function monthGrid(m) {
  const box = el("div", "month");
  const head = el("div", "month-head");
  const prev = el("button", "act", "‹");
  prev.addEventListener("click", () => showAgenda(m.previous));
  const next = el("button", "act", "›");
  next.addEventListener("click", () => showAgenda(m.next));
  head.append(prev, el("span", "month-title", m.title), next);
  box.append(head);

  const cells = el("div", "month-cells");
  for (const d of ["M", "T", "W", "T", "F", "S", "S"]) cells.append(el("span", "month-dow", d));
  for (const d of m.days) {
    const cls = ["month-day"];
    if (!d.in_month) cls.push("out");
    if (d.today) cls.push("today");
    if (d.date === agendaDay) cls.push("chosen");
    if (d.busy) cls.push("busy");
    const c = el("button", cls.join(" "), String(d.day));
    c.addEventListener("click", () => showAgenda(d.date));
    cells.append(c);
  }
  box.append(cells);
  return box;
}

/// An inline form under a row: words in, where they land shown as you type.
///
/// `preview` answers what the words would do without doing it; `commit` does
/// it. The button is enabled only once the preview has an answer, so a
/// misread "thursday" is seen before it is written.
function inlineForm(row, placeholder, preview, commit) {
  const form = el("div", "inline-form");
  const input = el("input");
  input.placeholder = placeholder;
  const said = el("span", "inline-said", "");
  const go = el("button", "act", "ok");
  go.disabled = true;
  let pending = 0;
  input.addEventListener("input", async () => {
    const mine = ++pending;
    go.disabled = true;
    said.textContent = "";
    if (!input.value.trim()) return;
    try {
      const answer = await preview(input.value);
      if (mine !== pending) return;
      said.textContent = answer ? `→ ${answer}` : "";
      go.disabled = false;
    } catch (e) {
      if (mine !== pending) return;
      said.textContent = String(e);
    }
  });
  go.addEventListener("click", async () => {
    await commit(input.value);
    showAgenda(agendaDay);
  });
  form.append(input, go, said);
  row.after(form);
  input.focus();
}

function agendaRow(r) {
  const row = el("div", `agenda-row${r.done ? " done" : ""}`);

  // What is due and what occurs, in one list and still told apart. A due date
  // has no hour — putting one on it would invent a deadline nobody wrote — so
  // the column that would hold a clock says what the row is instead.
  const when = r.due ? (r.was_due ? `was due ${r.was_due}` : "due") : r.all_day ? "" : clockOf(r.start_ms);
  row.append(el("span", r.due ? "when due" : "when", when));

  const body = el("span", "what", r.text || "(no readable text)");
  row.append(body);

  if (r.moved_from) {
    row.append(el("span", "shift", `moved from ${dayAndClock(r.moved_from)}`));
  }

  // Why this has no time. Each button names what it will write, because the
  // answer goes into the text.
  if (r.unclear_hour != null) {
    const h = r.unclear_hour;
    row.append(el("span", "shift", `"at ${h}" — which?`));
    for (const [label, pm] of [[`${h}am`, false], [`${h}pm`, true]]) {
      const b = el("button", "act", label);
      b.addEventListener("click", async () => {
        await call("settle_hour", { name: r.slug, pm });
        showAgenda(agendaDay);
      });
      row.append(b);
    }
  }

  // On both days it could mean, and on neither until answered. Each button
  // names the day it will write into the text.
  if (r.unclear_day && r.unclear_day.length === 2) {
    row.append(el("span", "shift", "which day?"));
    r.unclear_day.forEach((label, which) => {
      const b = el("button", "act", label);
      b.addEventListener("click", async () => {
        await call("settle_day", { name: r.slug, which });
        showAgenda(agendaDay);
      });
      row.append(b);
    });
  }

  // One occurrence, changed where it is shown (§3.5): move this one, skip it,
  // put it back, or change this and every later one.
  if (!r.due && (r.series || r.moved_from || r.skipped)) {
    const move = el("button", "act", "move…");
    move.addEventListener("click", () =>
      inlineForm(
        row,
        "when instead — thursday 10am",
        (text) => call("preview_move", { name: r.slug, occ: r.occ, text }),
        (text) => call("move_occurrence", { name: r.slug, occ: r.occ, text }),
      ),
    );
    row.append(move);
    if (r.skipped || r.moved_from) {
      const back = el("button", "act", r.skipped ? "restore" : "move back");
      back.addEventListener("click", async () => {
        await call("restore_occurrence", { name: r.slug, occ: r.occ });
        showAgenda(agendaDay);
      });
      row.append(back);
    }
    if (!r.skipped) {
      const skip = el("button", "act", "skip");
      skip.addEventListener("click", async () => {
        await call("skip_occurrence", { name: r.slug, occ: r.occ });
        showAgenda(agendaDay);
      });
      row.append(skip);
    }
    if (r.series) {
      const later = el("button", "act", "this and later…");
      later.addEventListener("click", () =>
        inlineForm(
          row,
          `from ${agendaDay} on — every tuesday at 6pm`,
          async (rule) => (rule.trim() ? `repeats as "${rule}" from ${agendaDay}` : ""),
          (rule) => call("split_series", { name: r.slug, from: agendaDay, rule }),
        ),
      );
      row.append(later);
    }
  }

  // A shifted occurrence says why. §4.4: a time that moves on its own with no
  // explanation is what makes somebody distrust the whole system.
  if (r.shift === "gap") {
    row.append(el("span", "shift", "moved by a daylight-saving gap"));
  } else if (r.shift === "ambiguous") {
    row.append(el("span", "shift", "the clock struck this hour twice"));
  }

  // Completion is per occurrence, not per series. Ticking Tuesday says nothing
  // about Wednesday, and un-ticking has to be possible because a mistaken tick
  // is a thing that happens.
  if (r.due) {
    // A task's own state, not an occurrence's. §3.6 keeps them apart: a task
    // has state, a series has state per instance and nothing accumulates
    // between them — and sending one through the other would either lose the
    // due date or invent an occurrence no rule produced.
    const tick = el("button", "act", r.done ? "✓ done" : "mark done");
    tick.addEventListener("click", async () => {
      await call("set_task_done", { name: r.slug, done: !r.done });
      showAgenda(agendaDay);
    });
    row.append(tick);
  } else if (!r.all_day) {
    const tick = el("button", "act", r.done ? "✓ done" : "mark done");
    tick.addEventListener("click", async () => {
      await call("set_occurrence", {
        name: r.slug,
        occ: r.occ,
        done: !r.done,
      });
      showAgenda(agendaDay);
    });
    row.append(tick);
  }

  row.addEventListener("click", (e) => {
    if (e.target.tagName !== "BUTTON") openNote(r.slug);
  });
  return row;
}

/// The timed grid: hours down the side, occurrences placed on them.
///
/// §4.4 asks for this shape rather than a list, and the reason is overlap. A
/// list of times can only say that two things start at nine; a grid shows that
/// one runs until eleven and the other is over by half past — which is the
/// question somebody actually has when they look at a day.
///
/// The columns are not decided here. `agenda` packs them, so the command line
/// and this page resolve a day the same way and a change to the rule cannot
/// reach one surface and miss the other.
function grid(rows) {
  const wrap = el("div", "grid");
  if (!rows.length) {
    wrap.append(el("p", "empty", "Nothing at a particular time."));
    return wrap;
  }

  // The hours drawn are the hours in use, with a little room either side.
  //
  // A fixed midnight-to-midnight grid spends most of its height on hours
  // nobody has anything in, and pushes the part being read off the screen. An
  // occurrence outside the usual working day still pulls the grid open to
  // include it — the alternative is drawing a day that omits something on it,
  // which is worse than any amount of scrolling.
  const starts = rows.map((r) => hourOf(r.start_ms));
  const ends = rows.map((r) => hourOf(r.end_ms || r.start_ms) + 1);
  const first = Math.max(0, Math.min(...starts) - 1);
  const last = Math.min(24, Math.max(...ends) + 1);

  const span = (last - first) * 60;
  wrap.style.height = `${span * PIXELS_PER_MINUTE}px`;

  for (let h = first; h < last; h++) {
    const line = el("div", "hour");
    line.style.top = `${(h - first) * 60 * PIXELS_PER_MINUTE}px`;
    line.append(el("span", "hour-label", `${String(h).padStart(2, "0")}:00`));
    wrap.append(line);
  }

  // The blocks go in a container inset past the hour labels, rather than being
  // offset one by one. An absolutely positioned child resolves against its
  // ancestor's *padding* box, so padding on the grid moves the labels and not
  // the blocks — which put every block on top of the label beside it, and then
  // put the labels off the left of the window when the offset was applied
  // twice. One inset box, and the percentages inside it mean what they say.
  const lanes = el("div", "lanes");
  for (const r of rows) lanes.append(block(r, first));
  wrap.append(lanes);
  return wrap;
}

/// How tall a minute is. One place, because the hour lines and the blocks have
/// to agree or an occurrence is drawn against the wrong hour.
const PIXELS_PER_MINUTE = 1.4;

/// The room an occurrence with no written end takes.
///
/// This is `UNWRITTEN` in ipc.rs and the two have to be the same number. The
/// packer reserves fifteen minutes for an end-less occurrence so that two
/// meetings at nine collide instead of being stacked into one column; if the
/// page then draws them taller than that, occurrences the packer called
/// separate are drawn touching — which is what happened at 13:43 and 14:00 on
/// a real day, seventeen minutes apart and overlapping on screen.
///
/// It is still a mark rather than a length. The promoter refuses to invent
/// §3.3's "30m default", because a wrong end time is harder to notice than a
/// missing one, and a block claiming half an hour would put that invention back
/// on the screen looking like something somebody wrote.
const UNWRITTEN_MINUTES = 15;
const NO_END_HEIGHT = UNWRITTEN_MINUTES * PIXELS_PER_MINUTE;

function block(r, firstHour) {
  const b = el("div", `block${r.done ? " done" : ""}${r.end_ms ? "" : " open"}`);

  const startMin = minutesOf(r.start_ms) - firstHour * 60;
  b.style.top = `${startMin * PIXELS_PER_MINUTE}px`;
  b.style.height = r.end_ms
    ? `${Math.max(NO_END_HEIGHT, (minutesOf(r.end_ms) - minutesOf(r.start_ms)) * PIXELS_PER_MINUTE)}px`
    : `${NO_END_HEIGHT}px`;

  // Side by side where they overlap, full width where they do not. The hour
  // labels live in the grid's left padding, so a block at 0% starts clear of
  // them rather than on top of the one it shares an hour with.
  const width = 100 / r.columns;
  b.style.left = `${r.column * width}%`;
  b.style.width = `${width - 1}%`;

  b.append(el("span", "block-when", clockOf(r.start_ms)));
  b.append(el("span", "block-what", r.text || "(no readable text)"));

  // A shifted occurrence says so here too. §4.4: a time that moves on its own
  // with no explanation is what makes somebody distrust the whole system, and
  // the grid is where a moved time is most visible and least explained.
  if (r.shift === "gap") {
    b.append(el("span", "shift", "moved by a daylight-saving gap"));
  } else if (r.shift === "ambiguous") {
    b.append(el("span", "shift", "the clock struck this hour twice"));
  }

  b.addEventListener("click", async () => {
    await call("set_occurrence", { name: r.slug, occ: r.occ, done: !r.done });
    showAgenda(agendaDay);
  });
  b.title = r.done ? "click to un-tick" : "click to mark done";
  return b;
}

/// "Mon 12 Oct 14:00", in the reader's zone.
function dayAndClock(ms) {
  const d = new Date(ms);
  const day = d.toLocaleDateString(undefined, { weekday: "short", day: "numeric", month: "short" });
  return `${day} ${clockOf(ms)}`;
}

function hourOf(ms) {
  return new Date(ms).getHours();
}

function minutesOf(ms) {
  const d = new Date(ms);
  return d.getHours() * 60 + d.getMinutes();
}

function clockOf(ms) {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

function today() {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

function shiftDay(day, by) {
  const d = new Date(`${day}T12:00:00`);
  d.setDate(d.getDate() + by);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

// ---------------------------------------------------------------- helpers ---

function el(tag, cls, text) {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text !== undefined) n.textContent = text;
  return n;
}

function toast(message) {
  const t = document.getElementById("toast");
  t.textContent = message;
  t.hidden = false;
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => { t.hidden = true; }, 4000);
}

/// A store error, as distinct from a fault in this page.
///
/// The difference decides what the reader is told, and getting it wrong told
/// somebody their window was broken when the store was merely busy being
/// upgraded. A `call` that fails rethrows this, the view catches it and says
/// the store could not be read, and the poll tries again a moment later — which
/// is usually all it takes.
class StoreError extends Error {}

async function call(name, args) {
  try {
    return await invoke(name, args);
  } catch (e) {
    // Shown rather than swallowed. A surface that silently does nothing when a
    // write fails is worse than one that refuses loudly.
    toast(String(e));
    throw new StoreError(String(e));
  }
}

/// Draw a store error, and let the next poll try again.
///
/// Not `fatal`. That is for a fault in this page — a TypeError, a bad
/// assumption — where the page really is broken and staying still is the right
/// thing. A store that says no is a different situation and often a passing
/// one: a projection being rebuilt after an upgrade, another process holding
/// the writer for a moment. Reporting it as breakage is both alarming and
/// wrong, and the evidence was somebody watching a "broken" window fix itself
/// a few seconds later.
function notReady(e) {
  view.replaceChildren();
  view.append(el("h2", null, "Could not read the store"));
  view.append(el("p", null, String(e.message || e)));
  view.append(el("p", "hint", "Trying again — this usually clears on its own."));
}

/// The day a note belongs to, in the zone it was captured in.
///
/// Not the reader's zone. A note made at 23:40 in one place must not move to a
/// different day when read in another — which is exactly when situational cues
/// matter most and the text is least likely to be findable.
function captureDay(note) {
  const d = new Date(note.captured_ms);
  try {
    return new Intl.DateTimeFormat(undefined, {
      timeZone: note.tz || "UTC",
      weekday: "short", year: "numeric", month: "short", day: "numeric",
    }).format(d);
  } catch {
    // An unknown zone. Falling back keeps the note readable rather than
    // dropping it out of the timeline entirely.
    return new Intl.DateTimeFormat(undefined, {
      weekday: "short", year: "numeric", month: "short", day: "numeric",
    }).format(d);
  }
}

function stamp(ms, tz) {
  try {
    return new Intl.DateTimeFormat(undefined, {
      timeZone: tz || "UTC", dateStyle: "medium", timeStyle: "short",
    }).format(new Date(ms));
  } catch {
    return new Date(ms).toLocaleString();
  }
}

// ------------------------------------------------------------------ notes ---

/// One row. Every state is rendered — never blank, never omitted.
///
/// A `div` acting as a button rather than a `button`, so that a list's items
/// can carry real checkboxes. A checkbox inside a `button` is invalid HTML and
/// behaves differently between engines, which is the concrete form of the
/// objection this row used to carry — "nesting a second control inside it would
/// make one gesture ambiguous". The ambiguity is resolved by the checkbox
/// stopping its own click rather than by refusing to have one.
///
/// Which the row was already doing. The tag buttons a few lines down have been
/// nested here all along, stopping their own clicks for exactly this reason —
/// a `button` inside a `button`, just as invalid, and never noticed because it
/// mostly works. So the objection was already contradicted by the code above
/// it; what this changes is making the nesting legitimate as well as working.
///
/// Keyboard access is kept by hand because a `div` does not bring it. Losing
/// Enter and Space on every row in the timeline to gain a checkbox would be a
/// poor trade, and an easy one to make without noticing.
function noteRow(note, onOpen) {
  const row = el("div", "note");
  row.setAttribute("role", "button");
  row.tabIndex = 0;
  row.addEventListener("click", () => onOpen(note.slug));
  row.addEventListener("keydown", (e) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      onOpen(note.slug);
    }
  });

  const head = el("div", "name");
  head.append(note.alias ? `${note.alias}  ·  ${note.slug}` : note.slug);

  if (note.state !== "ordinary") {
    const s = el("span", `state ${note.state}`);
    s.textContent =
      note.state === "redacted"
        ? `destroyed ${note.redacted_ms ? stamp(note.redacted_ms, note.tz) : ""}`.trim()
        : note.state === "diverged"
          ? `${note.divergences + 1} versions`
          : note.state;
    head.append(s);
  }
  if (note.pinned) head.append(el("span", "state", "pinned"));
  // What the archive made of it, where it made anything.
  //
  // The phone has drawn this since the projection change; this window took the
  // same three fields across the IPC boundary and dropped them on the floor,
  // so a task, an event and a series all rendered as the line somebody typed.
  // §3.1 makes the type a promotion rather than a choice, and a surface that
  // never shows the promotion is one where that distinction does not exist.
  if (note.descriptor) head.append(el("span", "descriptor", note.descriptor));
  for (const t of note.tags) {
    const b = el("button", "tag", `#${t}`);
    b.addEventListener("click", (e) => {
      // The row beneath is itself clickable, and a tag is not a way to open the
      // note it happens to sit on.
      e.stopPropagation();
      openMentions("#", t);
    });
    head.append(b);
  }
  row.append(head);

  const body = el("div", "body");
  if (note.text !== null && note.text !== undefined) {
    body.textContent = note.text;
  } else {
    // Two different facts, and a reader shown the wrong one has been told the
    // wrong story about their own archive.
    body.className = "body absent";
    body.textContent =
      note.state === "redacted"
        ? "(content destroyed)"
        : note.state === "withheld"
          ? "(content withheld — queued for destruction)"
          : "(no readable text)";
  }
  // The items, where this capture names any.
  //
  // Drawn on the row rather than behind a click, for the reason the phone
  // draws them: the sigil syntax is an accelerant for *writing*, and a reader
  // left to decode `#groceries: milk, eggs, bread` by eye is doing the
  // recognizer's job by hand.
  //
  // Tickable here, which is what the requirement asks for: an item is
  // completed where it is drawn, without navigating to a separate surface
  // first.
  //
  // This row used to refuse, and the reason was real rather than fussy — it was
  // a `button`, and a checkbox inside a button is invalid HTML. The row is a
  // `div` with a button's role now, so the nesting is legitimate, and the
  // ambiguity of one gesture meaning two things is resolved where it arises:
  // the checkbox stops its own click from reaching the row.
  //
  // By position within this capture, never by the item's words. Two items
  // reading the same are ordinary on a shopping list.
  if (note.items && note.items.length) {
    const list = el("ul", "items tickable");
    note.items.forEach((item, ord) => {
      const li = el("li", item.done ? "item done" : "item");
      const box = el("input");
      box.type = "checkbox";
      box.checked = item.done;
      // The click, not the change: the row listens for `click`, so this is
      // what stops one gesture meaning two things. Stopping the change as well
      // was belt-and-braces that made a test unable to tell which of the two
      // mattered.
      box.addEventListener("click", (e) => e.stopPropagation());
      box.addEventListener("change", async () => {
        await call("tick_item_at", {
          name: note.slug,
          ord,
          done: box.checked,
        });
        // The row shows it immediately: the write succeeded, so drawing it
        // done is true rather than optimistic.
        //
        // Said explicitly because `refresh` does not cover every view this row
        // appears in — it redraws the timeline, health and the agenda, and not
        // search, a tag page or an entity page. Without this line a tick on one
        // of those would leave the strike-through showing the state from before
        // the click.
        li.classList.toggle("done", box.checked);
        refresh();
      });
      li.append(box, el("span", "text", item.text));
      list.append(li);
    });
    row.append(list);
  }

  row.append(body);
  return row;
}

function contextStrip(ambient, onFilter) {
  const strip = el("div", "ctx");
  for (const [field, value] of ambient) {
    const b = el("button", null, `${field}: ${value}`);
    b.title = `Show everything from ${field} ${value}`;
    b.addEventListener("click", (e) => { e.stopPropagation(); onFilter(field, value); });
    strip.append(b);
  }
  return strip;
}

// --------------------------------------------------------------- timeline ---

async function showTimeline() {
  const mine = beginRender();
  current = "timeline";
  view.replaceChildren();

  await renderPending();

  const notes = await call("timeline", { limit: 500 });
  if (stale(mine)) return;
  if (notes.length === 0) {
    view.append(emptyState());
    return;
  }

  let day = null;
  for (const n of notes) {
    const d = captureDay(n);
    if (d !== day) {
      view.append(el("div", "day", d));
      day = d;
    }
    view.append(noteRow(n, openNote));
  }
}

/// What to show before anything has been captured.
///
/// Names the gesture rather than offering a button. No sample content: a seeded
/// note is one somebody has to identify as unreal and then delete, and it makes
/// the first real capture the second thing in the archive.
function emptyState() {
  const box = el("div", "empty");
  box.append(el("p", null, "Nothing captured yet."));
  const p = el("p");
  p.append("Press the capture chord anywhere and type. From a terminal, ");
  p.append(el("code", null, "capture + your first note"));
  p.append(".");
  box.append(p);
  box.append(el("p", null, "There is no button here that makes a note — the gesture is the same everywhere."));
  return box;
}

async function showFiltered(field, value) {
  const mine = beginRender();
  current = "filtered";
  view.replaceChildren();
  const back = el("button", "act", "← everything");
  back.addEventListener("click", showTimeline);
  view.append(back);
  view.append(el("h2", null, `${field}: ${value}`));

  const notes = await call("filter_by_context", { field, value });
  if (stale(mine)) return;
  if (notes.length === 0) view.append(el("p", "empty", "Nothing else from there."));
  for (const n of notes) view.append(noteRow(n, openNote));
}

// -------------------------------------------------------------- note page ---

/// Open a note, remembering where you came from.
///
/// The navigating entry point. `showNote` is the drawing one and is called
/// directly by anything redrawing the page it is already on — an edit, a tag —
/// so that changing a note does not push a copy of it into the history.
function openNote(name) {
  // Following a link from inside the wiki keeps you inside it. Without this
  // every link in the pane would throw away the sidebar and land on a bare note
  // page, which is the opposite of what a pane is for — and it would happen on
  // the first click, since the note page's own links are the main way through a
  // wiki.
  if (current === "wiki") return openWiki({ kind: "note", name });
  visit({ view: "note", name });
  return showNote(name);
}

async function showNote(name, into = view) {
  // `into` is what makes a note page embeddable. Inside the wiki pane it is the
  // pane's content area rather than the whole surface, so the sidebar beside it
  // survives — the alternative was a second copy of this function that would
  // have drifted from it within a week.
  const embedded = into !== view;
  const mine = beginRender();
  if (!embedded) current = "note";
  currentNote = name;
  drawHistory();
  const page = await call("note", { name });
  if (stale(mine)) return;
  const n = page.note;
  into.replaceChildren();

  if (!embedded) {
    const bar = el("div", "row nav");
    const back = el("button", "act", "← timeline");
    back.addEventListener("click", showTimeline);
    bar.append(back);
    into.append(bar);
  }

  const title = el("h2", null, n.alias ? `${n.alias}  ·  ${n.slug}` : n.slug);
  into.append(title);

  const text = el("div", "text");
  if (n.text !== null && n.text !== undefined) {
    // Rendered from the pieces the store split, never by scanning the text
    // here. Every piece goes in as text — a note is stored verbatim and may
    // contain anything at all, so a link's name is set with textContent like
    // everything else and never as markup.
    for (const p of page.body) {
      if (p.mention) {
        // Followable in the middle of a sentence, with its sigil kept: it is
        // what somebody typed and how they will look for it, and hiding it
        // would make the page a paraphrase of the note rather than the note.
        const m = el("button", "mention", `${p.mention}${p.text}`);
        m.addEventListener("click", () => openMentions(p.mention, p.text));
        text.append(m);
      } else if (!p.link) {
        text.append(document.createTextNode(p.text));
      } else if (p.state === "ok") {
        const a = el("button", "link", p.text);
        a.addEventListener("click", () => openNote(p.target));
        text.append(a);
      } else {
        // Shown, not hidden, and showing the name that failed. §4.3: an
        // unexplained absence costs more trust than an explained one, and a
        // link quietly repointed at different content is worse than either.
        //
        // Two ways to have nowhere to go, and they are not the same thing to
        // the reader. A moved name is a warning — something it used to reach is
        // now something else. An unresolved one is ordinary: forward references
        // are a normal way to write, and drawing one in alarm colours would
        // make writing one feel like a mistake.
        const b = el("span", `link ${p.state}`, `[[${p.text}]]`);
        b.title =
          p.state === "moved"
            ? "this name no longer means what it did when it was written"
            : "nothing has this name yet";
        text.append(b);
      }
    }
  } else {
    text.className = "text absent";
    text.textContent = n.redacted
      ? "(content destroyed)"
      : n.state === "withheld"
        ? "(content withheld — queued for destruction)"
        : "(no readable text)";
  }
  into.append(text);

  // Tags, which the note page did not show at all — they were on the timeline
  // row and nowhere else, so the page for a note said less about it than the
  // list it came from.
  if (n.tags.length) {
    const box = el("div", "tagline");
    for (const t of n.tags) {
      const b = el("button", "tag", `#${t}`);
      b.addEventListener("click", () => openMentions("#", t));
      box.append(b);
    }
    into.append(box);
  }

  // What the note currently *is*, beside what was captured.
  //
  // §3.1.4 keeps these apart deliberately: the text is the episodic record, the
  // fields are what actually fires, and neither is regenerated from the other.
  // Showing both is what makes drift visible — the text saying "every day at 7"
  // while the fields say weekdays is a thing you can see rather than a
  // discrepancy hiding in a form.
  if (page.promotion) {
    const box = el("div", "promotion");
    box.append(el("span", "kind", page.promotion.kind));
    box.append(el("span", "descriptor", page.promotion.descriptor));
    if (page.promotion.pinned) {
      const p = el("span", "pinned-by-hand", "yours");
      p.title = "You set this. No later reparse will change it.";
      box.append(p);
    } else if (page.promotion.confidence !== null) {
      const c = el("span", "confidence", `${page.promotion.confidence}%`);
      c.title = "How sure the recognizer was.";
      box.append(c);
    }
    into.append(box);
  }

  // The list this capture names, worked through here.
  //
  // The timeline draws these read-only and says why: a row is a button that
  // opens the note, and a control nested inside it makes one gesture
  // ambiguous. This is the page that comment points at, and until now it did
  // not draw them at all — so the one surface meant for working through a list
  // was the one surface without it.
  //
  // Ticked by position. Two items reading the same are ordinary on a shopping
  // list, and their words cannot tell them apart.
  if (page.items && page.items.length) {
    const list = el("ul", "items tickable");
    page.items.forEach((item, ord) => {
      const li = el("li", item.done ? "item done" : "item");
      const box = el("input");
      box.type = "checkbox";
      box.checked = item.done;
      box.addEventListener("change", async () => {
        await call("tick_item_at", { name: n.slug, ord, done: box.checked });
        showNote(n.slug);
      });
      li.append(box, el("span", "text", item.text));
      list.append(li);
    });
    into.append(list);
    const left = page.items.filter((i) => !i.done).length;
    into.append(
      el(
        "p",
        "hint",
        left === 0
          ? `All ${page.items.length} done.`
          : `${left} of ${page.items.length} left.`,
      ),
    );
  }

  // A reading it had and did not act on, and what to do about it.
  //
  // Two different offers wearing one sentence. Accepting a *type* annotates and
  // leaves the words alone; inserting a missing colon rewrites them. The button
  // says which, because making them look alike would teach that accepting a
  // suggestion never touches your text — which would then be false.
  if (page.suggestion) {
    const box = el("div", "suggestion");
    box.append(el("p", null, page.suggestion));

    const missingColon = page.suggestion.includes("would make it one");
    const act = el("button", "act", missingColon ? "Add the colon" : "Yes, that is what it is");
    act.addEventListener("click", async () => {
      if (missingColon) {
        const text = await call("fix_list", { name: n.slug });
        toast(`Now a list: ${text}`);
      } else {
        const became = await call("accept_suggestion", { name: n.slug });
        toast(`Taken as ${became}. "Read it again" on this page undoes it.`);
      }
      showNote(n.slug);
    });
    box.append(act);
    into.append(box);
  }

  // An hour written without am or pm. Asked rather than guessed: "gym every
  // monday at 2" was a series at 2 am. Each button names what it will write,
  // because the answer goes into the text.
  if (page.unclear_hour != null) {
    const h = page.unclear_hour;
    const box = el("div", "suggestion");
    box.append(el("p", null, `No time yet — "at ${h}" could be either. Which?`));
    for (const [label, pm] of [[`${h}am`, false], [`${h}pm`, true]]) {
      const b = el("button", "act", label);
      b.addEventListener("click", async () => {
        const text = await call("settle_hour", { name: n.slug, pm });
        toast(`Now reads: ${text}`);
        showNote(n.slug);
      });
      box.append(b);
    }
    into.append(box);
  }

  // A day left open between two: on neither until answered.
  if (page.unclear_day && page.unclear_day.length === 2) {
    const box = el("div", "suggestion");
    box.append(el("p", null, "Not on the calendar yet — this could be either day. Which?"));
    page.unclear_day.forEach((label, which) => {
      const b = el("button", "act", label);
      b.addEventListener("click", async () => {
        const text = await call("settle_day", { name: n.slug, which });
        toast(`Now reads: ${text}`);
        showNote(n.slug);
      });
      box.append(b);
    });
    into.append(box);
  }

  // A correction outlives every edit. That is the point of one, and it was a
  // trap on 2026-10-13: an accepted suggestion held the wrong day through every
  // fix to the text. So the way back is shown wherever one is in force.
  if (page.promotion && page.promotion.pinned) {
    const box = el("div", "suggestion");
    box.append(el("p", null, "You set what this is, so editing the text will not change it."));
    const b = el("button", "act", "Read it again from the text");
    b.addEventListener("click", async () => {
      const became = await call("reread", { name: n.slug });
      toast(`Read again: now ${became}.`);
      showNote(n.slug);
    });
    box.append(b);
    into.append(box);
  }

  const meta = el("p", "day");
  meta.textContent = `captured ${stamp(n.captured_ms, n.tz)}`;
  if (n.updated_ms !== n.captured_ms) {
    meta.textContent += `  ·  updated ${stamp(n.updated_ms, n.tz)}`;
  }
  into.append(meta);

  const ambient = await call("ambient", { name }).catch(() => []);
  if (ambient.length) into.append(contextStrip(ambient, showFiltered));

  if (n.divergences > 0) {
    // Reported, never prompted. A divergence is information about the note,
    // not a task to clear before continuing.
    const d = el("div", "cond attention");
    d.textContent =
      `${n.divergences + 1} versions were written independently. Both are kept.`;
    const dismiss = el("button", "act", "Dismiss");
    dismiss.addEventListener("click", async () => {
      const n2 = await call("resolve", { name: n.slug });
      toast(`Dismissed ${n2} competing version(s). Both remain in the log.`);
      showNote(n.slug);
    });
    const dismissRow = el("div", "row");
    dismissRow.append(dismiss);
    d.append(dismissRow);
    into.append(d);
  }

  into.append(linkList("Links to", page.links, (l) => {
    if (l.broken) {
      const li = el("li");
      const span = el("span", "broken", l.name);
      li.append(span, ` — was ${l.target || "a note that is gone"}`);
      return li;
    }
    // Nothing to open, so nothing that looks openable. The same defect as in
    // the body: a name matching no note was drawn as a link and went nowhere.
    if (!l.target) {
      const li = el("li");
      li.append(el("span", "link unresolved", l.name), " — nothing has this name yet");
      return li;
    }
    const li = el("li");
    const a = el("a", null, l.name);
    a.addEventListener("click", () => openNote(l.target));
    li.append(a);
    return li;
  }));

  into.append(linkList("Linked from", page.backlinks, (b) => {
    const li = el("li");
    const a = el("a", null, b.slug);
    a.addEventListener("click", () => openNote(b.slug));
    li.append(a, ` — as ${b.name}`);
    return li;
  }));

  into.append(actions(n, into, page.promotion));
}

function linkList(heading, items, render) {
  const box = el("div", "links");
  if (items.length === 0) return box;
  box.append(el("h2", null, heading));
  const ul = el("ul");
  for (const i of items) ul.append(render(i));
  box.append(ul);
  return box;
}

/// Edit the text in place, with `[[` completion.
///
/// A browser `prompt` was what this was, and it is a single-line box for
/// multi-line prose with no way to see the note while changing it. Worse for
/// the wiki layer specifically: a slug ends in ten base32 characters nobody
/// will retype, so writing a link by hand meant going and finding the note to
/// copy its name — the filing ritual §3.7 exists to avoid. §4.3 specifies `[[`
/// autocomplete for exactly this reason.
///
/// The editor replaces the text block rather than opening over it, so the note
/// stays where it was on the page and Escape puts it back untouched.
/** Show a page and open its editor, for one just started or just made. */
async function openPageForWriting(name) {
  await showNote(name);
  const page = await call("note", { name });
  editNote(page.note);
}

function editNote(n) {
  const box = document.querySelector(".text");
  if (!box) return;

  const area = el("textarea", "editor");
  area.value = n.text || "";
  // Sized to the text, so a one-line note does not get a page-tall box and a
  // long one does not get a peephole.
  area.rows = Math.min(20, Math.max(3, area.value.split("\n").length + 1));

  const hint = el(
    "p",
    "hint",
    "⌘/Ctrl+Enter saves · Escape cancels · [[ page · @ person · ^ project · # tag · \"- \" checklist line",
  );
  const suggestions = el("div", "completions");
  suggestions.hidden = true;

  const done = () => showNote(n.slug);

  const save = async () => {
    if (area.value === (n.text || "")) return done();
    await call("edit", { name: n.slug, text: area.value });
    toast("Edited. The previous version is kept.");
    done();
  };

  // What is being completed: the text between an unclosed `[[` and the caret.
  //
  // Deliberately narrow. It gives up at a newline or a closing bracket, so a
  // note that merely contains brackets somewhere earlier does not put the
  // editor into a completion it cannot leave.
  // `[[` runs to the caret; `@`, `^` and `#` run back to the start of the
  // word (pages, 2026-10-13). The phone's editor keeps the same rule.
  const openLink = () => {
    if (area.selectionStart !== area.selectionEnd) return null;
    const upto = area.value.slice(0, area.selectionStart);
    const at = upto.lastIndexOf("[[");
    if (at >= 0) {
      const after = upto.slice(at + 2);
      if (!after.includes("]]") && !after.includes("\n")) {
        return { sigil: "[[", at, prefix: after };
      }
    }
    let start = upto.length;
    while (start > 0 && !/\s/.test(upto.charAt(start - 1))) start -= 1;
    const word = upto.slice(start);
    const sigil = word.charAt(0);
    if (!["@", "^", "#"].includes(sigil)) return null;
    const prefix = word.slice(1);
    // A tag's list form is finished once its colon is typed.
    if (prefix.includes(":")) return null;
    return { sigil, at: upto.length - word.length, prefix };
  };

  let offers = [];
  let chosen = 0;

  const draw = () => {
    suggestions.replaceChildren();
    suggestions.hidden = offers.length === 0;
    offers.forEach((name, i) => {
      const b = el("button", i === chosen ? "offer on" : "offer", name);
      b.addEventListener("click", () => accept(name));
      suggestions.append(b);
    });
  };

  const accept = (name) => {
    const open = openLink();
    if (!open) return;
    const before = area.value.slice(0, open.at);
    const after = area.value.slice(area.selectionStart);
    const inserted = open.sigil === "[[" ? `[[${name}]]` : `${open.sigil}${name} `;
    area.value = `${before}${inserted}${after}`;
    const caret = before.length + inserted.length;
    area.setSelectionRange(caret, caret);
    offers = [];
    draw();
    area.focus();
  };

  const look = async () => {
    const open = openLink();
    if (!open) {
      offers = [];
      draw();
      return;
    }
    offers = await call("completions", { sigil: open.sigil, prefix: open.prefix });
    chosen = 0;
    draw();
  };

  area.addEventListener("input", look);
  area.addEventListener("keydown", (e) => {
    if (offers.length) {
      // The completion list owns these keys while it is open, and nothing else
      // does. Enter inserting a newline while a list is showing is the thing
      // that makes an autocomplete infuriating.
      if (e.key === "ArrowDown") {
        chosen = (chosen + 1) % offers.length;
        draw();
        e.preventDefault();
        return;
      }
      if (e.key === "ArrowUp") {
        chosen = (chosen - 1 + offers.length) % offers.length;
        draw();
        e.preventDefault();
        return;
      }
      if (e.key === "Enter" || e.key === "Tab") {
        accept(offers[chosen]);
        e.preventDefault();
        return;
      }
      if (e.key === "Escape") {
        offers = [];
        draw();
        e.preventDefault();
        return;
      }
    }
    if (e.key === "Escape") {
      done();
      e.preventDefault();
    }
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      save();
      e.preventDefault();
    }
  });

  const row = el("div", "row");
  const ok = el("button", "act", "Save");
  ok.addEventListener("click", save);
  const cancel = el("button", "act", "Cancel");
  cancel.addEventListener("click", done);
  row.append(ok, cancel);

  box.replaceWith(area);
  area.after(suggestions, hint, row);
  area.focus();
  area.setSelectionRange(area.value.length, area.value.length);
}

/// Everything that can be done to a note, from the note.
///
/// The design is explicit that management should not require a terminal:
/// muting exists so that suppressing something is *cheaper* than destroying it,
/// and that gradient inverts if suppression needs a shell while destruction is
/// a documented command.
function actions(n, into = view, promotion = null) {
  const row = el("div", "row");

  const add = (label, fn, cls) => {
    const b = el("button", cls ? `act ${cls}` : "act", label);
    b.addEventListener("click", fn);
    row.append(b);
    return b;
  };

  add("Edit", () => editNote(n));

  // A capture grown into a page stops being read for dates; while it is one,
  // the way back is offered instead.
  if (promotion && promotion.kind === "page") {
    add("Make it a capture again", async () => {
      const became = await call("reread", { name: n.slug });
      toast(`A capture again: now ${became}.`);
      showNote(n.slug);
    });
  } else {
    add("Make this a page", async () => {
      await call("make_page", { name: n.slug });
      await openPageForWriting(n.slug);
    });
  }

  add("Tag", async () => {
    const tag = prompt("Tag:");
    if (!tag) return;
    await call("tag", { name: n.slug, tag });
    showNote(n.slug);
  });

  if (n.tags.length) {
    add("Untag", async () => {
      const tag = prompt(`Remove which tag?\n\n${n.tags.join("  ")}`);
      if (!tag) return;
      await call("untag", { name: n.slug, tag });
      showNote(n.slug);
    });
  }

  // The one filing gesture in the interface, and it says what it costs: a page
  // moved into the wrong branch is still in the timeline, in search and behind
  // every link — which is the whole reason this was allowed to exist.
  add("Move…", async () => {
    const under = prompt(
      "Put this under which page?\n\nLeave empty to move it back to the top.",
      "",
    );
    if (under === null) return;
    await call("set_parent", { name: n.slug, under: under.trim() || null });
    toast(under.trim() ? `Under ${under.trim()}.` : "Back at the top.");
    showNote(n.slug, into);
  });

  // Only on a series, because it is the only thing a rule can be split at.
  // §3.5: changing the rule in place would rewrite what the schedule *was*,
  // and a log that revises its own history is an unreliable narrator — which
  // is the failing the whole system exists to compensate for.
  if (promotion && promotion.kind === "series") {
    add("Change from…", async () => {
      const from = prompt(
        "Change it from which date onward?\n\n" +
          "The first occurrence on or after this day, written as 2026-09-15.\n" +
          "Everything before it keeps the rule it already had.",
      );
      if (!from) return;
      const rule = prompt(
        `From ${from.trim()} onward, how often?\n\n` +
          'Written the way you would capture it — "every tuesday at 9".',
        promotion.descriptor || "",
      );
      if (!rule) return;

      await call("split_series", {
        name: n.slug,
        from: from.trim(),
        rule: rule.trim(),
      });
      toast(
        "Split. What came before is unchanged, and what follows is a new " +
          "note with the same text.",
      );
      showTimeline();
    });
  }

  // Saying what a capture is, which the recognizer deliberately declines to
  // guess at. A bare task verb is below the floor by design — "send sarah the
  // migration doc" is a task and the parser will not say so — and until now the
  // only way to correct that from the window was not to.
  //
  // Distinct from accepting a suggestion, which takes the reading the
  // recognizer *made* and was not confident enough to apply. That control needs
  // there to be a reading; this one is for when there is none, which is exactly
  // when somebody needs it.
  // Not on a page, which goes back by "Make it a capture again" and says so.
  if (promotion?.kind !== "task" && promotion?.kind !== "page") {
    add("Make a task…", async () => {
      // Empty is an ordinary answer, not a refusal: deciding a thing is owed
      // and deciding when it is owed are different decisions, and most tasks
      // arrive without a day. A prompt that insisted would push people into
      // inventing dates.
      const due = prompt(
        "Due when? YYYY-MM-DD, or leave empty for no date.",
        "",
      );
      if (due === null) return;
      await call("as_task", { name: n.slug, due: due.trim() || null });
      toast("A task now. The text is unchanged.");
      showNote(n.slug, into);
    });
  }

  // Demotion, which the requirement names specifically: a thing wrongly
  // promoted is as common as a thing wrongly left alone.
  //
  // Offered for a series too — demoting one needs no rule, which is what
  // separates it from the other direction. Making something a series means
  // writing a recurrence, and a free-text prompt for one is a worse surface
  // than the capture box, which already reads "every weekday at 9".
  if (promotion && promotion.kind && promotion.kind !== "note" && promotion.kind !== "page") {
    add("Make a note", async () => {
      await call("set_kind", { name: n.slug, kind: "note" });
      toast("A plain note now. The text is unchanged.");
      showNote(n.slug, into);
    });
  }

  add("Rename", async () => {
    const alias = prompt("A chosen name (blank clears it):", n.alias || "");
    if (alias === null) return;
    await call("set_alias", { name: n.slug, alias: alias.trim() || null });
    showNote(n.slug);
  });

  add(n.pinned ? "Unpin" : "Pin", async () => {
    await call("pin", { name: n.slug, pinned: !n.pinned });
    showNote(n.slug);
  });

  if (!n.muted) {
    add("Mute", async () => {
      await call("mute", { name: n.slug });
      toast("Muted. It stays in the timeline and leaves search.");
      showNote(n.slug);
    });
  } else {
    // The way back, from the same place. §2.6: reversible from any device.
    add("Unmute", async () => {
      await call("unmute", { name: n.slug });
      toast("No longer muted. It appears in search again.");
      showNote(n.slug);
    });
  }

  if (!n.redacted) add("Destroy", () => confirmRedaction(n), "danger");
  return row;
}

// -------------------------------------------------------------- redaction ---

/// The only destructive confirmation in the system.
///
/// It names what will go, item by item, and its accept button stays disabled
/// until the note's name has been typed. A count is dismissible in a way a name
/// is not, and dismissing dialogs is a reflex this one has to survive — which
/// is why Enter and Escape do not reach it.
function confirmRedaction(note) {
  const box = document.getElementById("confirm");
  const items = document.getElementById("confirm-items");
  const field = document.getElementById("confirm-name");
  const go = document.getElementById("confirm-go");
  say({
    title: "Destroy this content?",
    why: "This cannot be undone once the window elapses.",
    hint: "Type the note's name to confirm:",
    go: "Destroy",
    cancel: "Keep it",
  });

  items.replaceChildren();
  const li = el("li");
  li.append(el("strong", null, note.slug));
  if (note.text) li.append(document.createElement("br"), note.text.split("\n")[0]);
  items.append(li);

  field.value = "";
  go.disabled = true;
  box.hidden = false;
  field.focus();

  const check = () => { go.disabled = field.value.trim() !== note.slug; };
  const close = () => {
    box.hidden = true;
    field.removeEventListener("input", check);
    go.onclick = null;
  };

  field.addEventListener("input", check);
  document.getElementById("confirm-cancel").onclick = () => { close(); };
  go.onclick = async () => {
    close();
    const id = await call("redact", { name: note.slug });
    toast(`Queued. Cancel within the window — id ${id}.`);
    showTimeline();
  };
}

/// The one dialog's words, set per use.
///
/// It was written for destruction and says so in the markup. Reusing it
/// without changing them would have asked "Destroy this content?" over a
/// device retirement, which deletes nothing — and this is the one screen in
/// the system where being precise is the entire job.
function say({ title, why, hint, go, cancel }) {
  document.getElementById("confirm-title").textContent = title;
  document.getElementById("confirm-why").textContent = why;
  document.getElementById("confirm-hint").textContent = hint;
  document.getElementById("confirm-go").textContent = go;
  document.getElementById("confirm-cancel").textContent = cancel;
}

/// The pending countdown, refreshed on a tick.
async function renderPending() {
  const pending = await call("pending_redactions");
  for (const p of pending) {
    const box = el("div", "pending");
    const left = el("div", "left");
    const update = () => {
      const secs = Math.max(0, Math.round((p.execute_after_ms - Date.now()) / 1000));
      const m = Math.floor(secs / 60);
      left.textContent =
        secs > 0
          ? `${p.slug} will be destroyed in ${m}m ${secs % 60}s`
          : `${p.slug} is due to be destroyed`;
    };
    update();
    box.append(left);

    const cancel = el("button", "act", "Cancel");
    cancel.addEventListener("click", async () => {
      await call("cancel_redaction", { id: p.id });
      toast("Cancelled. Nothing was destroyed.");
      showTimeline();
    });
    const cancelRow = el("div", "row");
    cancelRow.append(cancel);
    box.append(cancelRow);
    view.prepend(box);

    // Live, because a countdown that only moves when you navigate is not a
    // countdown.
    const timer = setInterval(() => {
      if (!document.body.contains(left)) { clearInterval(timer); return; }
      update();
    }, 1000);
  }
}

/// Everything naming a person or a project.
///
/// §4.3's entity page: a filtered timeline, and nothing to create or maintain.
/// A page exists for every name the moment it is written, which is what makes
/// this usable on the first day rather than after months of curation — and it
/// is why there is no "new entity" anywhere in the interface.
/// Open an entity page, remembering where you came from.
function openMentions(sigil, value) {
  if (current === "wiki") return openWiki({ kind: "mentions", sigil, value });
  visit({ view: "mentions", sigil, value });
  return showMentions(sigil, value);
}

async function showMentions(sigil, value, into = view) {
  const embedded = into !== view;
  const mine = beginRender();
  if (!embedded) current = "mentions";
  currentMention = { sigil, value };
  drawHistory();
  into.replaceChildren();

  if (!embedded) {
    const bar = el("div", "row");
    const back = el("button", "act", "← timeline");
    back.addEventListener("click", showTimeline);
    bar.append(back);
    into.append(bar);
  }

  into.append(el("h2", null, `${sigil}${value}`));
  entityActions(sigil, value, into);

  // One page for three sigils, from two tables. A tag is an OR-Set with its own
  // events and a mention is part of the text — a real difference underneath,
  // and none at all to somebody following one.
  const notes =
    sigil === "#"
      ? await call("tagged", { tag: value })
      : await call("mentioning", { sigil, value });
  if (stale(mine)) return;

  if (!notes.length) {
    // Reachable: a page can be opened from a note whose text is then edited,
    // or from one that has since been muted. Saying so is better than an empty
    // page that reads as a fault.
    into.append(el("p", "empty", "Nothing names this now."));
    return;
  }

  into.append(
    el("p", "hint", `${notes.length} note${notes.length === 1 ? "" : "s"}`),
  );
  for (const n of notes) into.append(noteRow(n, openNote));
}

/// Renaming, merging and binding, on the page they are about.
///
/// §8.4 makes the window the first-party surface, and identity is the one thing
/// extraction cannot get right on its own: two spellings are indistinguishable
/// from two people by phrasing alone, so somebody has to say. A correction that
/// needed a command line would be the gradient §8.4 refuses.
///
/// Tags are left out. They have no identity of their own here — the sidebar
/// lists them from the OR-Set as well as from the text — so renaming or merging
/// one would be a different operation wearing the same word.
function entityActions(sigil, value, into) {
  if (sigil === "#") return;

  const bar = el("div", "row");

  const rename = el("button", "act", "rename");
  rename.addEventListener("click", async () => {
    const display = prompt(
      `What to call ${sigil}${value}.\n\n` +
        "The notes are unchanged, and every spelling still reaches this page.",
    );
    if (!display) return;
    await call("rename_entity", { sigil, value, display });
    showMentions(sigil, value, into);
  });

  const merge = el("button", "act", "merge into…");
  merge.addEventListener("click", async () => {
    const into_name = prompt(
      `Merge ${sigil}${value} into which one?\n\n` +
        "As written, without the sigil. Nothing moves: both names reach one\n" +
        "page, and every note still says exactly what it said.",
    );
    if (!into_name) return;
    await call("merge_entities", { sigil, from: value, into: into_name });
    // The survivor is the page to land on, which is the one just named.
    openMentions(sigil, into_name);
  });

  const bind = el("button", "act", "backing note…");
  bind.addEventListener("click", async () => {
    const note = prompt(
      `The note that *is* ${sigil}${value}, by name.\n\n` +
        "Blank unbinds. It keeps being whatever it already was — a task stays\n" +
        "a task, and stays wherever tasks appear.",
    );
    if (note === null) return;
    await call("bind_backing", { sigil, value, note: note || null });
    showMentions(sigil, value, into);
  });

  // A page of its own, written about them and shown above every note naming
  // them (pages, 2026-10-14). Binding replaces whatever note was bound.
  const page = el("button", "act", "start a page…");
  page.addEventListener("click", async () => {
    const title = prompt(`Title of ${sigil}${value}'s page:`, value);
    if (!title || !title.trim()) return;
    const made = await call("new_page_for", { sigil, value, title: title.trim() });
    if (!made.name) toast(`That name was taken, so this page is called ${made.slug}.`);
    await openPageForWriting(made.name || made.slug);
  });

  bar.append(rename, merge, bind, page);
  into.append(bar);
}

// ------------------------------------------------------------------- wiki ---

/// Where the wiki pane is, so leaving it and coming back returns you to it.
///
/// The point of a pane rather than a view: Timeline → a note → Agenda →
/// Timeline puts you back at the top of the timeline, which is right for a
/// timeline and wrong for a place you were reading. `null` is the pane with
/// nothing open yet.
let wikiAt = null;

/// Which tag the Lists view is showing, kept so the nav button returns to it.
let listTag = null;

/// The wiki: a sidebar of everything there is to browse, and a page beside it.
///
/// The sidebar is not maintained by anybody. Pages are the notes somebody
/// aliased — the one act that says "this is a thing I will come back to" — and
/// people, projects and tags are whatever the notes happen to say. There is
/// nothing to file into and nothing to prune, which is §3.7's argument applied
/// to a sidebar: a list that needs tending is a list that stops being tended.
/// The items written under a tag, gathered from every capture that wrote them.
///
/// The surface whose purpose is working through something, and the one the
/// window did not have. The note page draws a *single capture's* items, which
/// answers a different question — a grocery list is spread over however many
/// times somebody thought of something, and gathering them is the whole point
/// of a collection.
///
/// Ticked by `(slug, ord)`: the same call the note page makes, with the capture
/// taken from the row rather than from the page. Position alone is meaningless
/// here, because position 0 exists in every capture under the tag — which is
/// why the line carries where it came from.
/// What is outstanding, in the three bands.
///
/// The window was the first-party surface without this, and the gap was not a
/// missing convenience. A task occupies no time (§3.6) so the agenda cannot
/// hold it, and the agenda's due band wants a due date — so an *undated* task,
/// which is the ordinary kind, was reachable here only by scrolling a timeline
/// ordered by when things were captured rather than by whether they are owed.
///
/// Beside Agenda deliberately: the two answer the halves of one question, and
/// §3.6's whole discriminator is that a thing you attend and a thing you owe
/// are different.
async function showTasks() {
  const mine = beginRender();
  current = "tasks";
  view.replaceChildren();

  const rows = await call("tasks", {});
  if (stale(mine)) return;

  const frame = el("div", "tasks");
  view.append(frame);

  if (!rows.length) {
    // Plainly, and not as a failure. Nothing outstanding is the good state.
    frame.append(el("p", "empty", "Nothing outstanding."));
    return;
  }

  // In this order, and the undated band present rather than folded into the
  // others: a task with no due date is the ordinary case, because the
  // recognizer refuses to promote a bare task verb and a person saying "send
  // sarah the migration doc" is not saying when.
  const bands = [
    ["overdue", "Overdue"],
    ["due", "Due"],
    ["undated", "No date"],
  ];

  for (const [key, label] of bands) {
    const mine_ = rows.filter((r) => r.band === key);
    if (!mine_.length) continue;
    // The agenda's own band label, not a second one that looks nearly the
    // same: these are the same idea on two surfaces.
    frame.append(el("div", "band-label", label));
    const ul = el("ul", "task-rows");
    for (const row of mine_) {
      const li = el("li", "task-row");
      const box = el("input");
      box.type = "checkbox";
      box.checked = false;
      box.title = "done";
      box.addEventListener("change", async () => {
        // The same write the command line and the phone make. The view has the
        // slug to hand and could author the event itself; two paths to one
        // state is how two surfaces come to disagree about it.
        await call("set_task_done", { name: row.slug, done: box.checked });
        showTasks();
      });
      const open = el("button", "task-text", row.text || row.descriptor);
      open.addEventListener("click", () => openNote(row.slug));
      li.append(box, open);
      if (row.due) li.append(el("span", "task-due", row.due));
      ul.append(li);
    }
    frame.append(ul);
  }
}

async function showLists(tag) {
  const mine = beginRender();
  current = "lists";
  listTag = tag || null;
  view.replaceChildren();

  const index = await call("wiki_index", {});
  if (stale(mine)) return;

  const frame = el("div", "wiki lists");
  const side = el("aside", "sidebar");
  const page = el("div", "page");
  frame.append(side, page);
  view.append(frame);

  // The tags, which is where a list lives: §3.6 makes a list a tag with a
  // display order, so there is no separate thing to enumerate.
  side.append(el("div", "side-label", "tags"));
  for (const e of index.tags) {
    const b = el("button", "side-item", e.name);
    if (e.count !== null && e.count !== undefined) {
      b.append(el("span", "side-count", String(e.count)));
    }
    if (e.name === listTag) b.classList.add("on");
    b.addEventListener("click", () => showLists(e.name));
    side.append(b);
  }

  if (!index.tags.length) {
    page.append(el("p", "empty", "Nothing is tagged yet."));
    return;
  }
  if (!listTag) {
    page.append(el("p", "empty", "Pick a tag to see what is on it."));
    return;
  }

  const lines = await call("list", { tag: listTag });
  if (stale(mine)) return;

  page.append(el("h2", "page-title", listTag));
  if (!lines.length) {
    page.append(el("p", "empty", "Nothing written under this tag yet."));
    return;
  }

  // Said once, above the list, when it is true. A list of eleven things
  // gathered from four captures looks exactly like one list of eleven things,
  // and the difference matters the moment somebody goes to edit it — editing
  // "the list" means editing one of the four.
  const captures = new Set(lines.map((l) => l.slug));
  if (captures.size > 1) {
    page.append(
      el("p", "note-aside", `Gathered from ${captures.size} captures.`),
    );
  }

  const ul = el("ul", "items tickable");
  for (const line of lines) {
    const li = el("li", line.done ? "item done" : "item");
    // A task that joined the list by carrying the tag has no place in anybody
    // written order, so there is no position to tick it at. It is already a
    // task with its own completion, and a checkbox here would have nothing to
    // address.
    if (line.ord === null || line.ord === undefined) {
      li.append(el("span", "item-bullet", "\u00b7"));
    } else {
      const box = el("input");
      box.type = "checkbox";
      box.checked = line.done;
      box.addEventListener("change", async () => {
        await call("tick_item_at", {
          name: line.slug,
          ord: line.ord,
          done: box.checked,
        });
        showLists(listTag);
      });
      li.append(box);
    }
    const text = el("span", "text", line.text);
    li.append(text);
    const from = el("button", "item-from", line.slug);
    from.title = "the capture this came from";
    from.addEventListener("click", () => openNote(line.slug));
    li.append(from);
    ul.append(li);
  }
  page.append(ul);

  const left = lines.filter((l) => !l.done).length;
  page.append(
    el("p", "items-left", left === 0 ? "All done." : `${left} left.`),
  );
}

async function showWiki(at) {
  const mine = beginRender();
  current = "wiki";
  wikiAt = at || null;
  view.replaceChildren();

  const index = await call("wiki_index", {});
  if (stale(mine)) return;

  const frame = el("div", "wiki");
  const side = el("aside", "sidebar");
  const page = el("div", "page");
  frame.append(side, page);
  view.append(frame);

  const section = (label, entries, open, depth) => {
    if (!entries.length) return;
    side.append(el("div", "side-label", label));
    for (const e of entries) {
      const b = el("button", "side-item", e.name);
      if (depth) b.style.paddingLeft = `${0.4 + depth(e) * 0.8}rem`;
      if (e.count !== null && e.count !== undefined) {
        b.append(el("span", "side-count", String(e.count)));
      }
      if (same(open(e), wikiAt)) b.classList.add("on");
      b.addEventListener("click", () => openWiki(open(e)));
      side.append(b);
    }
  };

  // Start a page: a title names it and is its first line, and it opens for
  // writing. A page is never read for dates or types (pages, 2026-10-13).
  const start = el("button", "side-item new-page", "+ New page");
  start.addEventListener("click", async () => {
    const title = prompt("Title of the new page:");
    if (!title || !title.trim()) return;
    const made = await call("new_page", { title: title.trim(), under: null });
    if (!made.name) toast(`That name was taken, so this page is called ${made.slug}.`);
    await openPageForWriting(made.name || made.slug);
  });
  side.append(start);

  // Pages nest; the other three do not. A person is not under a project, and
  // arranging them as though they were would invent a relationship nobody
  // wrote — where a page under a page is one somebody said out loud.
  const { ordered, depthOf } = tree(index.pages);
  section("pages", ordered, (e) => ({ kind: "note", name: e.slug }), (e) => depthOf.get(e.slug) || 0);
  section("people", index.people, (e) => ({ kind: "mentions", sigil: "@", value: e.name }));

  section("projects", index.projects, (e) => ({ kind: "mentions", sigil: "^", value: e.name }));
  section("tags", index.tags, (e) => ({ kind: "mentions", sigil: "#", value: e.name }));

  if (!side.childNodes.length) {
    side.append(
      el("p", "empty", "Nothing to browse yet."),
      el(
        "p",
        "hint",
        "Pages are notes you have named, and the rest is whatever your notes say — write @someone, ^something or #anything, or give a note a name with Rename.",
      ),
    );
  }

  if (!wikiAt) {
    page.append(el("p", "empty", "Pick something on the left."));
    return;
  }

  // The pane draws a real note page or a real entity page, rather than its own
  // version of either. One implementation, so the wiki cannot drift from the
  // rest of the interface as either of them changes.
  if (wikiAt.kind === "note") await showNote(wikiAt.name, page);
  else await showMentions(wikiAt.sigil, wikiAt.value, page);
}

/// Arrange the pages into the order a tree is read in, with a depth each.
///
/// Depth-first from the pages that sit under nothing, alphabetically at every
/// level, which is what makes the list stable: a page keeps its position until
/// somebody moves it, and the sidebar is somewhere you can learn by shape.
///
/// A page whose parent is not itself a page — filed under an ordinary note, or
/// under one this replica does not hold — is drawn at the top rather than
/// hidden. It was moved somewhere, and a move that makes a page disappear from
/// the list it was moved within is the failure §3.7 warns about.
function tree(pages) {
  const byParent = new Map();
  const known = new Set(pages.map((p) => p.slug));
  for (const p of pages) {
    const key = p.parent && known.has(p.parent) ? p.parent : "";
    if (!byParent.has(key)) byParent.set(key, []);
    byParent.get(key).push(p);
  }

  const ordered = [];
  const depthOf = new Map();
  const walk = (key, depth) => {
    for (const p of byParent.get(key) || []) {
      // Guard rather than trust. The fold breaks cycles and refuses the
      // self-parent, so a loop should not reach here — but a sidebar that can
      // hang the window if it is wrong is not worth the assumption.
      if (depthOf.has(p.slug)) continue;
      depthOf.set(p.slug, depth);
      ordered.push(p);
      walk(p.slug, depth + 1);
    }
  };
  walk("", 0);

  // Anything the walk did not reach, for the same reason: a page that is not
  // drawn is a page somebody cannot find.
  for (const p of pages) if (!depthOf.has(p.slug)) {
    depthOf.set(p.slug, 0);
    ordered.push(p);
  }

  return { ordered, depthOf };
}

/// Move within the pane, recording it.
function openWiki(at) {
  visit({ view: "wiki", at });
  return showWiki(at);
}

// ----------------------------------------------------------------- review ---

/// A fixed small number of notes, and then it is over.
///
/// # What this surface deliberately does not have
///
/// No score, no streak, no count of how many sessions you have done, no
/// percentage. §2.7 removed the grade rather than the feature, and this is the
/// surface where a graded record would do the most harm: a recurring,
/// permanently recorded history of memory failures, delivered on a schedule the
/// user did not set, to somebody whose stated difficulty is memory.
///
/// The outcomes are *did something* and *not now*. Neither is a mark.
///
/// # Leaving is not deferring
///
/// An item you never touch records nothing beyond having been shown. Only the
/// button defers, because two deferrals end an item permanently — that is a
/// real consequence, and closing a window must not be able to trigger it.
async function showReview() {
  const mine = beginRender();
  current = "review";
  view.replaceChildren();

  if (!session) {
    // Cleared by removing it rather than by replacing the page a second time.
    // Two clears in one render is how a view ends up drawing over itself, and
    // the page has already been emptied above.
    const waiting = el("p", "empty", "Opening…");
    view.append(waiting);
    session = await call("review", {});
    if (stale(mine)) return;
    waiting.remove();
  }

  if (!session.items.length && !session.nagging.length) {
    view.append(el("h2", null, "Nothing to review"));
    view.append(
      el("p", "hint", "That is the ordinary state. Review offers what is due, and when nothing is, the session is over before it started."),
    );
    return;
  }

  if (session.items.length) {
    view.append(el("h2", null, `${session.items.length} note${session.items.length === 1 ? "" : "s"}, and then this is over.`));
  }

  for (const item of session.items) view.append(reviewCard(item));

  for (const n of session.nagging) view.append(naggingCard(n));
}

const WHY = {
  forward: "you said to come back to this",
  "never-engaged": "never looked at since capture",
  context: "captured somewhere like here",
};

function reviewCard(item) {
  const card = el("div", "note review");

  const open = el("button", "slug", item.slug);
  open.addEventListener("click", () => openNote(item.slug));
  card.append(open);

  card.append(el("p", "body", item.text || ""));
  card.append(el("p", "why", `— ${WHY[item.because] || item.because}`));

  // A reading the recognizer had and did not act on. One line, applied to
  // nothing: §3.1.1 is explicit that a low-confidence reading is an offer, and
  // an offer that quietly became a calendar entry is the failure the floor
  // exists to prevent.
  if (item.suggestion) card.append(el("p", "suggestion", item.suggestion));

  // An answered item keeps its answer on the session rather than in the page.
  //
  // The first version held it in the DOM, so leaving the pane and coming back
  // redrew the buttons on items that had already been answered — and pressing
  // one again would have written a second event. That is not cosmetic:
  // `engaged` and `deferred` accumulate, and two deferrals end an item
  // permanently. A round trip through the nav bar could have silently retired a
  // note the user pushed away exactly once.
  if (item.answered) {
    card.classList.add("done");
    card.append(el("p", "said", item.answered));
    return card;
  }

  const row = el("div", "row");
  const said = el("p", "said");
  said.hidden = true;

  const settle = (then) => {
    item.answered = then;
    row.hidden = true;
    said.textContent = then;
    said.hidden = false;
    card.classList.add("done");
  };

  const record = async (outcome, then) => {
    await call("reviewed", { name: item.slug, outcome });
    settle(then);
  };

  const engaged = el("button", "act", "Did something with it");
  engaged.addEventListener("click", () => record("engaged", "Noted."));

  const deferred = el("button", "act", "Not now");
  deferred.addEventListener("click", () =>
    record("deferred", "Pushed away. Twice and it stops being offered."),
  );

  // §8.7: muting applies to review, and asks for no reason. Reversible and
  // destroys nothing — the note stays exactly where it was.
  const mute = el("button", "act quiet", "Stop resurfacing this");
  mute.addEventListener("click", async () => {
    await call("mute", { name: item.slug });
    settle("Muted. It stays where it is; it just stops coming back.");
  });

  row.append(engaged, deferred, mute);
  card.append(row, said);
  return card;
}

/// The one question review asks, asked once.
///
/// §3.6: a series that has lapsed several times might be one that genuinely
/// accumulates — a dose rather than a walk — and the recognizer must never
/// decide that from phrasing. Both answers are offered, and the default of
/// nothing being owed is the one that needs no action.
function naggingCard(n) {
  const card = el("div", "note nagging");
  card.append(el("p", "body", `${n.slug} has lapsed several times.`));
  card.append(
    el("p", "why", "If missing it leaves something owed — a dose, an invoice — say so. Otherwise there is nothing to do: a missed day costs almost nothing."),
  );

  // Answered once, and it stays answered — the question is asked once, not once
  // per visit.
  if (n.answered) {
    card.classList.add("done");
    card.append(el("p", "said", n.answered));
    return card;
  }

  const row = el("div", "row");
  const said = el("p", "said");
  said.hidden = true;

  const settle = (then) => {
    n.answered = then;
    row.hidden = true;
    said.textContent = then;
    said.hidden = false;
    card.classList.add("done");
  };

  const owed = el("button", "act", "Missing it leaves something owed");
  owed.addEventListener("click", async () => {
    await call("carries_forward", { name: n.slug });
    settle("Noted. Missed instances will stay until they are dealt with.");
  });

  const fine = el("button", "act quiet", "Nothing is owed");
  fine.addEventListener("click", () => {
    // Nothing is written, because nothing changed. The default already says
    // this, and recording an event to confirm a default is how a log fills up
    // with facts about the system rather than facts about the user.
    settle("Then there is nothing to do.");
  });

  row.append(owed, fine);
  card.append(row, said);
  return card;
}

// ----------------------------------------------------------------- health ---

async function showHealth() {
  const mine = beginRender();
  current = "health";
  view.replaceChildren();

  const h = await call("health");
  if (stale(mine)) return;
  view.append(el("h2", null, "Store"));
  view.append(el("p", "healthy", `${h.notes} notes, ${h.events} events`));

  const attention = h.conditions.filter((c) => c.tier === "attention");
  const knowing = h.conditions.filter((c) => c.tier !== "attention");

  view.append(el("h2", null, "Needs attention"));
  if (attention.length === 0) {
    // Stated out loud. An empty section is ambiguous between "nothing is
    // wrong" and "nothing was checked", and the ambiguity is a cost this
    // system would be manufacturing for free.
    view.append(el("p", "healthy", "Nothing needs your attention."));
  }
  for (const c of attention) view.append(condition(c));

  if (knowing.length) {
    view.append(el("h2", null, "Worth knowing"));
    for (const c of knowing) view.append(condition(c));
  }

  // Devices. Shown even when there is only one, because "this is the only
  // device" is a fact worth being able to confirm — not seeing a second one
  // is otherwise indistinguishable from not having looked.
  view.append(el("h2", null, "Devices"));
  view.append(pairingRow());
  if (!h.peers || h.peers.length === 0) {
    view.append(el("p", "healthy", "No roster yet."));
  }
  for (const p of h.peers || []) view.append(peer(p));

  // Never reported as verified. This surface reads counters and opens no event.
  view.append(el("p", "unchecked", h.unchecked));
}

function peer(p) {
  const box = el("div", "cond worth-knowing");
  box.append(el("div", "peer-key", p.key));

  // The address and the last contact are separate facts, and a device can have
  // either without the other: one that only ever dials in is demonstrably
  // alive at no address this machine can call.
  let said;
  if (p.is_self) said = "this device";
  else if (p.last_seen_ms === null || p.last_seen_ms === undefined)
    said = p.addr ? `${p.addr} — never reached` : "never seen on any network";
  else said = `${p.addr || "no address"} — last reached ${ago(p.last_seen_ms)} ago`;

  box.append(el("div", null, said));

  // Not on this device, and not the last one standing. Retiring is how a lost
  // or stolen device stops being accepted, and the moment somebody needs it is
  // not a moment to be looking up a command.
  if (!p.is_self) {
    const b = el("button", "act danger", "Retire");
    b.addEventListener("click", () => confirmRetire(p));
    box.append(b);
  }
  return box;
}

/// Adding a device: a code here, typed there.
///
/// The window shows the code and then waits, because the answer arrives
/// whenever somebody walks to the other device — which is the gesture, not a
/// delay to be engineered away. It is polled rather than pushed for the reason
/// the whole reader is: something that depends on being told shows the wrong
/// thing for ever if the message is lost, where something that looks for
/// itself is at worst late.
function pairingRow() {
  const box = el("div", "cond worth-knowing");
  const line = el("div");
  box.append(line);

  const start = el("button", "act", "Add a device");
  start.addEventListener("click", async () => {
    start.disabled = true;
    const s = await call("start_pairing");
    render(s);
    poll();
  });
  box.append(start);

  const render = (s) => {
    if (s.admitted) {
      line.textContent = `Paired with ${s.admitted}. It receives the archive on the next sync.`;
      start.disabled = false;
      start.textContent = "Add another";
      return true;
    }
    if (s.failed) {
      line.textContent = s.failed;
      start.disabled = false;
      start.textContent = "Try again";
      return true;
    }
    if (s.code) {
      line.textContent =
        `On the other device, run:  capture pair ${s.code} --at this machine` +
        "  —  the code stops working after one wrong attempt.";
      return false;
    }
    line.textContent = "No other devices yet.";
    return true;
  };

  const poll = () => {
    const timer = setInterval(async () => {
      const s = await call("pairing_state");
      if (render(s)) {
        clearInterval(timer);
        if (s.admitted) showHealth();
      }
    }, 1000);
  };

  render({ code: null, admitted: null, failed: null });
  return box;
}

/// Retiring asks for the key to be typed, like destruction asks for the name.
///
/// It is not destruction — nothing is deleted, and the device's past events
/// stay valid — but it is the other thing in the system that cannot be undone
/// by clicking again: coming back needs two devices and a co-signature. A
/// count is dismissible in a way a typed key is not.
function confirmRetire(p) {
  const box = document.getElementById("confirm");
  const items = document.getElementById("confirm-items");
  const field = document.getElementById("confirm-name");
  const go = document.getElementById("confirm-go");
  say({
    title: "Stop accepting this device?",
    why: "Nothing is deleted. It stops being accepted from where it is now.",
    hint: "Type the device's key to confirm:",
    go: "Retire",
    cancel: "Leave it",
  });

  items.replaceChildren();
  const li = el("li");
  li.append(el("strong", null, p.key));
  li.append(
    document.createElement("br"),
    "Stops being accepted from where it is now. Its past events stay — they " +
      "were legitimately authored. Coming back needs another device to vouch.",
  );
  items.append(li);

  field.value = "";
  go.disabled = true;
  box.hidden = false;
  field.focus();

  const check = () => {
    go.disabled = field.value.trim() !== p.key;
  };
  const close = () => {
    box.hidden = true;
    field.removeEventListener("input", check);
    go.onclick = null;
  };

  field.addEventListener("input", check);
  document.getElementById("confirm-cancel").onclick = () => close();
  go.onclick = async () => {
    close();
    await call("retire_device", { key: p.key });
    toast("Retired. Other devices learn of it as they sync.");
    showHealth();
  };
}

function ago(ms) {
  const days = Math.floor(ms / 86400000);
  if (days > 0) return `${days} day${days === 1 ? "" : "s"}`;
  const hours = Math.floor(ms / 3600000);
  if (hours > 0) return `${hours} hour${hours === 1 ? "" : "s"}`;
  const mins = Math.floor(ms / 60000);
  return `${mins} minute${mins === 1 ? "" : "s"}`;
}

function condition(c) {
  const box = el("div", `cond ${c.tier}`);
  box.append(el("div", null, c.text));
  if (c.acknowledgeable) {
    const b = el("button", "act", "I have dealt with this");
    b.addEventListener("click", async () => {
      await call("acknowledge_exports");
      toast("Acknowledged. Nothing was checked — this records your word for it.");
      showHealth();
    });
    const row = el("div", "row");
    row.append(b);
    box.append(row);
  }
  return box;
}

// ------------------------------------------------------------------- wiring ---

// Only the buttons that name a view. Back and forward live in the same row and
// are not places — wiring them here would clear the highlight and then fall
// through to the timeline, so the history controls would navigate to something
// other than history.
document.querySelectorAll("nav button[data-view]").forEach((b) => {
  b.addEventListener("click", () => {
    document.querySelectorAll("nav button[data-view]").forEach((o) => o.classList.remove("on"));
    b.classList.add("on");
    if (b.dataset.view === "wiki") showWiki(wikiAt);
    else if (b.dataset.view === "health") showHealth();
    else if (b.dataset.view === "agenda") showAgenda(today());
    else if (b.dataset.view === "review") showReview();
    else if (b.dataset.view === "lists") showLists(listTag);
    else if (b.dataset.view === "tasks") showTasks();
    else showTimeline();
  });
});

document.getElementById("back").addEventListener("click", () => {
  const to = history.back.pop();
  if (!to) return;
  const from = here();
  if (from) history.forward.push(from);
  go(to);
});

document.getElementById("forward").addEventListener("click", () => {
  const to = history.forward.pop();
  if (!to) return;
  const from = here();
  if (from) history.back.push(from);
  go(to);
});

const q = document.getElementById("q");
let searchTimer;
q.addEventListener("input", () => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(async () => {
    const query = q.value.trim();
    if (!query) { showTimeline(); return; }
    current = "search";
    view.replaceChildren();
    const hits = await call("search", { query });
    view.append(el("h2", null, hits.length ? "Results" : `Nothing matches ${query}`));
    for (const n of hits) view.append(noteRow(n, openNote));
  }, 150);
});

// The latency half of the due-work rule. Every process startup is the floor
// that makes the guarantee; a resident window is what makes a deadline land
// near its time rather than whenever somebody next runs a command.
setInterval(() => {
  invoke("tick").then((n) => {
    if (n > 0) refresh();
  }).catch(() => {});
}, 30000);

// Notice captures. Until this existed the window only ever redrew when a
// deadline elapsed, so a note captured with the chord did not appear in a
// window that was already open — and the store was fine, which is the worst
// version of that: nothing to see, nothing wrong, and no reason given.
//
// Half a second, matching `watch::POLL_INTERVAL`. It is a display latency
// rather than a correctness bound; the cost is two integer reads.
setInterval(() => {
  invoke("changed").then((moved) => {
    if (moved) refresh();
  }).catch(() => {});
}, 500);

/// Redraw what is on screen, and nothing else.
///
/// Deliberately narrow. A refresh that rebuilt the page wholesale would throw
/// away a note being read, a half-typed search, or a redaction confirmation
/// waiting on its typed name — and it would do it under the user's hands,
/// because something changed somewhere else. Only the two surfaces that are
/// purely a view of the store redraw on their own; everything else is a place
/// the user is doing something, and waits.
function refresh() {
  if (document.getElementById("confirm").hidden === false) return;
  if (current === "timeline") showTimeline();
  else if (current === "health") showHealth();
  else if (current === "agenda") showAgenda(agendaDay);
  // Review is absent on purpose, and not merely unimplemented. Redrawing it
  // would re-render a session the user is part-way through, and — worse — the
  // `shown` events this pane writes are themselves a store change, so a review
  // that refreshed on change would refresh because of itself.
}

showTimeline();
