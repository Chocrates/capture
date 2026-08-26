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
window.addEventListener("unhandledrejection", (e) =>
  fatal((e.reason && (e.reason.message || e.reason)) || "a promise was rejected"),
);

let current = "timeline";

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

async function call(name, args) {
  try {
    return await invoke(name, args);
  } catch (e) {
    // Shown rather than swallowed. A surface that silently does nothing when a
    // write fails is worse than one that refuses loudly.
    toast(String(e));
    throw e;
  }
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
      note.state === "redacted" ? "(content destroyed)" : "(no readable text)";
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
  current = "timeline";
  view.replaceChildren();

  await renderPending();

  const notes = await call("timeline", { limit: 500 });
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
    view.append(noteRow(n, showNote));
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
  view.replaceChildren();
  const back = el("button", "act", "← everything");
  back.addEventListener("click", showTimeline);
  view.append(back);
  view.append(el("h2", null, `${field}: ${value}`));

  const notes = await call("filter_by_context", { field, value });
  if (notes.length === 0) view.append(el("p", "empty", "Nothing else from there."));
  for (const n of notes) view.append(noteRow(n, showNote));
}

// -------------------------------------------------------------- note page ---

async function showNote(name) {
  const page = await call("note", { name });
  const n = page.note;
  view.replaceChildren();

  const back = el("button", "act", "← timeline");
  back.addEventListener("click", showTimeline);
  view.append(back);

  const title = el("h2", null, n.alias ? `${n.alias}  ·  ${n.slug}` : n.slug);
  view.append(title);

  const text = el("div", "text");
  if (n.text !== null && n.text !== undefined) {
    text.textContent = n.text;
  } else {
    text.className = "text absent";
    text.textContent = n.redacted ? "(content destroyed)" : "(no readable text)";
  }
  view.append(text);

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
    const li = el("li");
    const a = el("a", null, l.name);
    a.addEventListener("click", () => showNote(l.target || l.name));
    li.append(a);
    return li;
  }));

  view.append(linkList("Linked from", page.backlinks, (b) => {
    const li = el("li");
    const a = el("a", null, b.slug);
    a.addEventListener("click", () => showNote(b.slug));
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

  add("Edit", async () => {
    const text = prompt("Replace the text with:", n.text || "");
    if (text === null) return;
    await call("edit", { name: n.slug, text });
    toast("Edited. The previous version is kept.");
    showNote(n.slug);
  });

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

// ----------------------------------------------------------------- health ---

async function showHealth() {
  current = "health";
  view.replaceChildren();

  const h = await call("health");
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
    b.dataset.view === "health" ? showHealth() : showTimeline();
  });
});

const q = document.getElementById("q");
let searchTimer;
q.addEventListener("input", () => {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(async () => {
    const query = q.value.trim();
    if (!query) { showTimeline(); return; }
    view.replaceChildren();
    const hits = await call("search", { query });
    view.append(el("h2", null, hits.length ? "Results" : `Nothing matches ${query}`));
    for (const n of hits) view.append(noteRow(n, showNote));
  }, 150);
});

// The latency half of the due-work rule. Every process startup is the floor
// that makes the guarantee; a resident window is what makes a deadline land
// near its time rather than whenever somebody next runs a command.
setInterval(() => {
  invoke("tick").then((n) => {
    if (n > 0 && current === "timeline") showTimeline();
  }).catch(() => {});
}, 30000);

showTimeline();
