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

/// Record where we are before going somewhere else.
///
/// Called by navigation, never by a redraw. `showNote` refreshing itself after
/// an edit must not push a second copy of the same note, or "back" walks
/// through the same page once per change made to it.
function visit(entry) {
    const here = current === "note" ? { view: "note", name: currentNote } : null;
    if (here && here.name !== entry.name) history.back.push(here);
    // A new destination ends the forward branch, exactly as it does in a
    // browser: the path you came back from is no longer the path you are on.
    history.forward.length = 0;
}

// The session on offer, held rather than re-fetched.
//
// Asking for a session is a write: being offered a note is the outcome `shown`,
// which moves `last_shown` and makes those notes ineligible for a day. Running
// `brain review` twice in a terminal is two deliberate acts and spending the
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
  view.append(bar);

  view.append(el("h2", null, agendaDay));

  const rows = await call("agenda", { day: agendaDay });
  if (stale(mine)) return;
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
  const allDay = rows.filter((r) => !r.due && r.all_day);
  const timed = rows.filter((r) => !r.due && !r.all_day);

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
function noteRow(note, onOpen) {
  const row = el("button", "note");
  row.addEventListener("click", () => onOpen(note.slug));

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
  for (const t of note.tags) head.append(el("span", "tag", `#${t}`));
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
  p.append(el("code", null, "brain + your first note"));
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
  visit({ view: "note", name });
  return showNote(name);
}

async function showNote(name) {
  const mine = beginRender();
  current = "note";
  currentNote = name;
  const page = await call("note", { name });
  if (stale(mine)) return;
  const n = page.note;
  view.replaceChildren();

  const bar = el("div", "row nav");

  const back = el("button", "act", "← timeline");
  back.addEventListener("click", showTimeline);
  bar.append(back);

  if (history.back.length) {
    const b = el("button", "act", "◀ back");
    b.addEventListener("click", () => {
      const to = history.back.pop();
      history.forward.push({ view: "note", name: currentNote });
      showNote(to.name);
    });
    bar.append(b);
  }
  if (history.forward.length) {
    const f = el("button", "act", "forward ▶");
    f.addEventListener("click", () => {
      const to = history.forward.pop();
      history.back.push({ view: "note", name: currentNote });
      showNote(to.name);
    });
    bar.append(f);
  }
  view.append(bar);

  const title = el("h2", null, n.alias ? `${n.alias}  ·  ${n.slug}` : n.slug);
  view.append(title);

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
        m.addEventListener("click", () => showMentions(p.mention, p.text));
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
  view.append(text);

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
    view.append(box);
  }

  // A reading it had and did not act on. One line, applied to nothing.
  if (page.suggestion) {
    view.append(el("p", "suggestion", page.suggestion));
  }

  const meta = el("p", "day");
  meta.textContent = `captured ${stamp(n.captured_ms, n.tz)}`;
  if (n.updated_ms !== n.captured_ms) {
    meta.textContent += `  ·  updated ${stamp(n.updated_ms, n.tz)}`;
  }
  view.append(meta);

  const ambient = await call("ambient", { name }).catch(() => []);
  if (ambient.length) view.append(contextStrip(ambient, showFiltered));

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
    view.append(d);
  }

  view.append(linkList("Links to", page.links, (l) => {
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

  view.append(linkList("Linked from", page.backlinks, (b) => {
    const li = el("li");
    const a = el("a", null, b.slug);
    a.addEventListener("click", () => openNote(b.slug));
    li.append(a, ` — as ${b.name}`);
    return li;
  }));

  view.append(actions(n));
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
function editNote(n) {
  const box = document.querySelector(".text");
  if (!box) return;

  const area = el("textarea", "editor");
  area.value = n.text || "";
  // Sized to the text, so a one-line note does not get a page-tall box and a
  // long one does not get a peephole.
  area.rows = Math.min(20, Math.max(3, area.value.split("\n").length + 1));

  const hint = el("p", "hint", "⌘/Ctrl+Enter saves · Escape cancels · [[ links");
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
  const openLink = () => {
    const upto = area.value.slice(0, area.selectionStart);
    const at = upto.lastIndexOf("[[");
    if (at < 0) return null;
    const after = upto.slice(at + 2);
    if (after.includes("]]") || after.includes("\n")) return null;
    return { at, prefix: after };
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
    area.value = `${before}[[${name}]]${after}`;
    const caret = before.length + name.length + 4;
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
    offers = await call("names", { prefix: open.prefix });
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
function actions(n) {
  const row = el("div", "row");

  const add = (label, fn, cls) => {
    const b = el("button", cls ? `act ${cls}` : "act", label);
    b.addEventListener("click", fn);
    row.append(b);
    return b;
  };

  add("Edit", () => editNote(n));

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
async function showMentions(sigil, value) {
  const mine = beginRender();
  current = "mentions";
  view.replaceChildren();

  const bar = el("div", "row");
  const back = el("button", "act", "← timeline");
  back.addEventListener("click", showTimeline);
  bar.append(back);
  view.append(bar);

  view.append(el("h2", null, `${sigil}${value}`));

  const notes = await call("mentioning", { sigil, value });
  if (stale(mine)) return;

  if (!notes.length) {
    // Reachable: a page can be opened from a note whose text is then edited,
    // or from one that has since been muted. Saying so is better than an empty
    // page that reads as a fault.
    view.append(el("p", "empty", "Nothing names this now."));
    return;
  }

  view.append(
    el("p", "hint", `${notes.length} note${notes.length === 1 ? "" : "s"}`),
  );
  for (const n of notes) view.append(noteRow(n, openNote));
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

  // Never reported as verified. This surface reads counters and opens no event.
  view.append(el("p", "unchecked", h.unchecked));
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

document.querySelectorAll("nav button").forEach((b) => {
  b.addEventListener("click", () => {
    document.querySelectorAll("nav button").forEach((o) => o.classList.remove("on"));
    b.classList.add("on");
    if (b.dataset.view === "health") showHealth();
    else if (b.dataset.view === "agenda") showAgenda(today());
    else if (b.dataset.view === "review") showReview();
    else showTimeline();
  });
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
