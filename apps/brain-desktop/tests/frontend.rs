//! Checks on the parts of the interface that are not Rust.
//!
//! # Why a text check rather than a browser
//!
//! Driving a real webview from a test needs a display, a driver and a protocol,
//! and it would still be checking the same handful of facts. These are cheap,
//! they run everywhere, and each one exists because of a specific failure.
//!
//! They are deliberately narrow. A test that tried to assert what the page
//! *looks like* would be re-implementing a browser badly; these assert only
//! things with a one-line answer and a real consequence.

use std::path::PathBuf;

fn ui(file: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("ui")
        .join(file);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("reading {}: {e}", p.display()))
}

/// The file with its line comments removed.
///
/// Checks for a forbidden construct have to ignore comments, or the comment
/// *explaining* why the construct is forbidden trips the check that enforces
/// it — and the obvious way to make the test pass is then to delete the
/// explanation.
fn ui_code(file: &str) -> String {
    ui(file)
        .lines()
        .map(|l| match l.find("//") {
            Some(i) if !l[..i].contains('"') => &l[..i],
            _ => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn hidden_beats_every_author_style() {
    // The bug this exists for: `#confirm { display: grid }` is an author-level
    // declaration, and the `hidden` attribute only works through the user-agent
    // stylesheet's `[hidden] { display: none }` — which author styles beat. The
    // redaction dialog was therefore visible from the moment the page loaded.
    //
    // The destructive confirmation appearing unprompted, on startup, before the
    // user has done anything, is the fastest possible way to train the reflex
    // it is built to survive.
    let css = ui("style.css");
    assert!(
        css.contains("[hidden] { display: none !important; }"),
        "the stylesheet must force `hidden` to win, or any element given a \
         `display` becomes permanently visible"
    );
}

#[test]
fn everything_marked_hidden_stays_hidden_until_asked_for() {
    // Every element the markup hides is one the interface reveals deliberately.
    // If a rule below gave one of them a `display`, it would be on screen from
    // the start — and the check above is what stops that.
    let html = ui("index.html");
    for id in ["confirm", "toast"] {
        assert!(
            html.contains(&format!("id=\"{id}\"")) && html.contains("hidden"),
            "{id} should start hidden"
        );
    }
}

#[test]
fn the_destroy_button_starts_disabled() {
    // A default-focused accept is what makes a dialog dismissible by reflex.
    // This one has to be typed into before it will do anything.
    let html = ui("index.html");
    assert!(
        html.contains("id=\"confirm-go\" disabled"),
        "the accept button must be disabled until the note's name is typed"
    );
}

#[test]
fn note_text_is_never_treated_as_markup() {
    // Captures are stored verbatim and are expected to contain anything at all,
    // including half-written HTML. Rendering any of it as markup would let a
    // note change the page displaying it.
    let js = ui_code("app.js");
    assert!(
        !js.contains("innerHTML"),
        "note content must go in through textContent"
    );
}

#[test]
fn nothing_on_screen_creates_a_note() {
    // An on-screen control becomes the path of least resistance, and once it
    // exists the capture gesture is no longer the same everywhere — which was
    // the entire point of having one gesture.
    let html = ui("index.html").to_lowercase();
    for word in ["new note", "add note", "create note", "+ note"] {
        assert!(
            !html.contains(word),
            "found a note-creating control: {word}"
        );
    }
}

#[test]
fn nothing_chains_onto_append() {
    // `Node.append` returns undefined, so `parent.append(child).append(x)`
    // throws on the second call. It reads like it works, and three of them
    // shipped.
    //
    // What made it expensive is where they were: one sat in the pending
    // redaction block, which `showTimeline` awaits *before* it asks for any
    // notes. So the window came up completely empty — no notes, no error,
    // nothing in the log — and only for stores that happened to have a
    // redaction queued. The store was never involved.
    let code = ui_code("app.js");
    for (i, line) in code.lines().enumerate() {
        let Some(after) = line.split_once(".append(").map(|(_, a)| a) else {
            continue;
        };
        assert!(
            !after.contains(").append("),
            "app.js:{} chains onto the result of append, which is undefined: {}",
            i + 1,
            line.trim()
        );
    }
}

#[test]
fn a_throw_in_the_page_is_shown_rather_than_swallowed() {
    // The corollary of the bug above. An exception used to leave a blank view
    // that was indistinguishable from an empty store — and the empty state
    // reads "Nothing captured yet", which is the one wrong answer.
    let code = ui_code("app.js");
    assert!(
        code.contains("unhandledrejection"),
        "a rejected promise must reach the page, not the void"
    );
    assert!(
        code.contains(r#"addEventListener("error""#),
        "a thrown error must reach the page, not the void"
    );
}

#[test]
fn the_api_the_page_needs_is_actually_injected() {
    // `window.__TAURI__` exists only if `withGlobalTauri` is set, and it
    // defaults to false. With it unset, app.js throws on its first statement
    // and the window renders nothing at all — the store, the commands and the
    // build are all fine, and the cause is one boolean in a file the page never
    // mentions.
    let conf = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json"),
    )
    .expect("reading tauri.conf.json");

    if ui_code("app.js").contains("window.__TAURI__") {
        assert!(
            conf.contains(r#""withGlobalTauri": true"#),
            "app.js reaches for window.__TAURI__, which tauri.conf.json does not ask to be injected"
        );
    }
}

#[test]
fn every_view_says_which_one_it_is() {
    // `refresh` reads `current` to decide whether redrawing is safe. A view
    // that replaces the page without setting it inherits whichever value was
    // there — and if that value is "timeline", the timeline reappears
    // underneath whatever the user was reading the next time anything changes.
    let code = ui_code("app.js");
    let replaces = code.matches("view.replaceChildren()").count();
    let declares = code.matches("current = \"").count();
    assert!(
        declares >= replaces,
        "{replaces} views replace the page but only {declares} say which view they are"
    );
}

#[test]
fn the_page_notices_captures_on_its_own() {
    // Until this existed the window redrew only when a redaction deadline
    // elapsed, so a note captured with the chord never appeared in a window
    // that was already open — and nothing looked wrong, which is the worst
    // version of it.
    let code = ui_code("app.js");
    assert!(
        code.contains(r#"invoke("changed")"#),
        "nothing asks the store whether anything moved"
    );
}

#[test]
fn a_render_cannot_be_overtaken_by_another() {
    // Every view clears the page, awaits the store, then appends. Two running
    // at once — a click and the refresh poll landing together — both clear and
    // both append, and the page shows everything twice. It looks like duplicate
    // data and no amount of looking at the store explains it.
    let code = ui_code("app.js");
    let awaits_then_appends = code.matches("beginRender()").count();
    assert!(
        awaits_then_appends >= 4,
        "only {awaits_then_appends} views take a render number; every async view needs one"
    );
    assert!(
        code.matches("stale(mine)").count() >= 4,
        "a view took a render number and never checked it was still the newest"
    );
}

#[test]
fn the_grid_does_not_invent_an_end_time() {
    // The promoter refuses to invent §3.3's "30m default", because a wrong end
    // time is harder to notice than a missing one. A block drawn half an hour
    // tall would put that invention straight back on the screen, where it looks
    // like something somebody wrote — so an occurrence with no written end is
    // drawn as a mark at its start and says so by its border.
    let code = ui_code("app.js");
    assert!(
        code.contains("NO_END_HEIGHT"),
        "an occurrence with no end has no distinct rendering"
    );
    assert!(
        code.contains("r.end_ms ?"),
        "the block height does not distinguish a written end from a missing one"
    );
    assert!(
        ui("style.css").contains(".block.open"),
        "nothing on screen tells a known length from an unknown one"
    );
}

#[test]
fn the_page_draws_an_unwritten_end_as_wide_as_the_packer_reserves() {
    // Two numbers that have to agree, in two languages. The packer reserves
    // fifteen minutes for an occurrence with no written end so that two
    // meetings at nine collide rather than being stacked into one column. Draw
    // them taller than that and occurrences the packer called separate are
    // drawn touching — which is exactly what happened on a real day, at 13:43
    // and 14:00, seventeen minutes apart and overlapping on screen.
    let rust = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/ipc.rs"),
    )
    .expect("ipc.rs is readable");
    assert!(
        rust.contains("const UNWRITTEN: i64 = 15 * 60 * 1000;"),
        "the packer's reservation changed; the page still assumes fifteen minutes"
    );
    assert!(
        ui_code("app.js").contains("const UNWRITTEN_MINUTES = 15;"),
        "the page no longer draws an unwritten end at the width the packer reserves"
    );
}

#[test]
fn the_page_does_not_decide_where_columns_go() {
    // Packing lives in `agenda`, so the command line and this page resolve a
    // day the same way and a change to the rule cannot reach one and miss the
    // other. A second implementation here would be a second answer.
    let code = ui_code("app.js");
    assert!(
        code.contains("r.columns") && code.contains("r.column"),
        "the grid does not use the columns it was given"
    );
    for invented in ["function pack", "overlaps(", "cluster"] {
        assert!(
            !code.contains(invented),
            "the page grew its own packing ({invented:?}), which is a second answer to one question"
        );
    }
}

#[test]
fn the_page_does_not_look_for_links_itself() {
    // The body arrives pre-split from `brain-core`, which is the same scan the
    // links table is built from. A second scanner here is a second answer to
    // one question: the note text would eventually disagree with the list of
    // links beneath it about what the note links to, and neither would be
    // visibly wrong.
    let code = ui_code("app.js");
    assert!(
        code.contains("page.body"),
        "the note body is not rendered from the pieces the store split"
    );
    // Looked for as a *scan*, not as an appearance: the page does render the
    // brackets back around a name it cannot follow, which is the point of
    // showing the raw text that failed.
    for scan in ["indexOf(\"[[\")", "split(\"[[\")", "\\[\\[", "match(/"] {
        assert!(
            !code.contains(scan),
            "the page looks for link syntax itself ({scan}), which is a second answer"
        );
    }
}

#[test]
fn a_link_with_nowhere_to_go_is_not_drawn_as_followable() {
    // Found by clicking one. A name matching no note was drawn as an ordinary
    // link and navigated nowhere — twice over, in the note body and in the
    // "links to" list. Three states, not two: it resolves, it moved, or nothing
    // has that name yet.
    let code = ui_code("app.js");
    assert!(
        code.contains(r#"p.state === "ok""#),
        "the body follows a link without checking whether it resolves"
    );
    assert!(
        code.contains("if (!l.target)"),
        "the links list still offers to open a name that matches nothing"
    );
    let css = ui("style.css");
    assert!(
        css.contains(".link.unresolved") && css.contains(".text .link.moved"),
        "a name that moved and a name nothing answers to look the same"
    );
}

#[test]
fn following_a_link_can_be_undone() {
    // §4.3 asks for this outright: "navigation keeps a back/forward history,
    // since click-through is the entire point". Three notes deep with no way
    // back except the timeline makes the third link one nobody clicks.
    let code = ui_code("app.js");
    assert!(
        code.contains("history.back") && code.contains("history.forward"),
        "there is no navigation history"
    );
}

#[test]
fn the_wiki_draws_the_real_note_page() {
    // One implementation, embedded. A second copy of the note page inside the
    // pane would drift from the first within a week — and the drift would be
    // silent, because both would keep working.
    let code = ui_code("app.js");
    assert!(
        code.contains("showNote(wikiAt.name, page)"),
        "the wiki renders its own idea of a note rather than the note page"
    );
    assert!(
        code.contains("async function showNote(name, into = view)"),
        "the note page cannot be drawn anywhere but the whole surface"
    );
}

#[test]
fn a_link_inside_the_wiki_stays_inside_it() {
    // Otherwise the first click throws away the sidebar and lands on a bare
    // note page, which is the opposite of what a pane is for — and it would
    // happen immediately, since a note's own links are the main way through a
    // wiki.
    let code = ui_code("app.js");
    let open = code
        .split("function openNote(")
        .nth(1)
        .expect("openNote exists");
    assert!(
        open.contains(r#"current === "wiki""#),
        "following a link from the wiki leaves it"
    );
}

#[test]
fn the_wiki_remembers_where_you_were() {
    // The difference between a pane and a view. Going to the agenda and back
    // must not return you to an empty page — that is right for a timeline and
    // wrong for something you were reading.
    let code = ui_code("app.js");
    assert!(
        code.contains("showWiki(wikiAt)"),
        "the wiki reopens at nothing rather than where you left it"
    );
}

#[test]
fn nothing_in_the_sidebar_is_maintained_by_hand() {
    // Pages are notes somebody aliased; people, projects and tags are whatever
    // the notes say. A control that added an entry would be the filing step
    // §3.7 exists to remove, and a list that needs tending stops being tended.
    let code = ui_code("app.js");
    for filing in ["addPage", "newPage", "createPage", "removeFromSidebar"] {
        assert!(
            !code.contains(filing),
            "the sidebar can be curated ({filing:?})"
        );
    }
    assert!(
        code.contains(r#"call("wiki_index""#),
        "the sidebar is not built from the store"
    );
}

#[test]
fn history_records_entity_pages_too() {
    // The first version recorded notes alone, which broke the one path entity
    // pages exist to create: timeline → `@sarah` → one of her notes left no way
    // back to `@sarah`, because the page in the middle was never written down.
    let code = ui_code("app.js");
    assert!(
        code.contains("function openMentions"),
        "an entity page is not a place you can come back from"
    );
    assert!(
        code.contains(r#"view === "mentions""#),
        "the history cannot describe an entity page"
    );
}

#[test]
fn the_history_controls_are_not_view_buttons() {
    // They share the navbar with Timeline, Agenda, Review and Health. Wired by
    // the same selector, clicking back would clear the highlight and fall
    // through to the timeline — so the history control would navigate to
    // something other than history.
    let code = ui_code("app.js");
    assert!(
        code.contains(r#"nav button[data-view]"#),
        "the view wiring is not scoped, so it also grabs back and forward"
    );
}

#[test]
fn a_control_that_cannot_be_used_is_still_visible() {
    // Disabled rather than hidden. A button that vanishes takes its meaning
    // with it and moves the row under the cursor, and this pair is unavailable
    // most of the time — which is exactly when somebody is learning what it is.
    let html = ui("index.html");
    assert!(
        html.contains(r#"id="back""#) && html.contains(r#"id="forward""#),
        "the history controls are not in the navbar"
    );
    assert!(
        html.contains("disabled"),
        "the controls start enabled, with nothing to go back to"
    );
    assert!(
        ui_code("app.js").contains("b.disabled = history.back.length === 0"),
        "nothing keeps the controls in step with the history"
    );
}

#[test]
fn redrawing_a_note_does_not_count_as_going_somewhere() {
    // `showNote` is the drawing entry point and `openNote` the navigating one.
    // If an edit or a tag redrew through `openNote`, "back" would walk through
    // one copy of the same note per change made to it.
    let code = ui_code("app.js");
    // Bounded to the function, by the next one declared after it. Taking
    // everything to the end of the file swept in the review pane, which
    // navigates through `openNote` correctly and should; bounding on a comment
    // marker instead does not work here, because `ui_code` strips comments.
    let after = code
        .split("function actions(")
        .nth(1)
        .expect("actions() exists");
    let end = after
        .find("\nfunction ")
        .into_iter()
        .chain(after.find("\nasync function "))
        .min()
        .unwrap_or(after.len());
    let actions = &after[..end];
    assert!(
        end < after.len(),
        "the slice ran to the end of the file, so this checks more than actions()"
    );
    assert!(
        !actions.contains("openNote("),
        "an action redraws through the navigating entry point, so back walks through edits"
    );
    assert!(
        actions.contains("showNote("),
        "this test would pass vacuously if actions redrew nothing"
    );
}

#[test]
fn editing_does_not_happen_in_a_browser_dialog() {
    // A `prompt` is a single-line box for multi-line prose, and it hides the
    // note while you change it. It also made the wiki layer unusable by hand: a
    // slug ends in ten base32 characters nobody will retype, so writing a link
    // meant leaving to find the note and copy its name — the filing ritual §3.7
    // exists to avoid.
    let code = ui_code("app.js");
    assert!(
        !code.contains("prompt(\"Replace the text"),
        "the editor is still a browser dialog"
    );
    assert!(
        code.contains("function editNote"),
        "there is no in-place editor"
    );
}

#[test]
fn completing_a_link_asks_the_store_for_names() {
    // §4.3 specifies `[[` autocomplete. The names come from the store rather
    // than from whatever happens to be on screen — the note you want to link to
    // is usually not one of the twenty in the timeline behind you.
    let code = ui_code("app.js");
    assert!(
        code.contains(r#"call("names""#),
        "completion does not ask the store, so it can only offer what is loaded"
    );
}

#[test]
fn the_completion_list_owns_enter_while_it_is_open() {
    // Enter inserting a newline while a list of offers is showing is the thing
    // that makes an autocomplete infuriating, and it is silent: you get a
    // broken link and a line break instead of the note you meant.
    let code = ui_code("app.js");
    let keydown = code
        .split("area.addEventListener(\"keydown\"")
        .nth(1)
        .expect("the editor handles keys");
    let guarded = keydown
        .split("if (offers.length)")
        .nth(1)
        .expect("the offers guard exists");
    assert!(
        guarded.contains("preventDefault"),
        "keys are not taken back from the textarea while offers are showing"
    );
}

#[test]
fn a_mention_is_followable_where_it_was_written() {
    // §4.3: entity links carry the load, because they exist without anyone
    // linking anything. A `@sarah` that is not clickable is a string.
    let code = ui_code("app.js");
    assert!(
        code.contains("function showMentions"),
        "there is no entity page"
    );
    assert!(
        code.contains("p.mention"),
        "mentions in a note's text are not followable"
    );
}

#[test]
fn all_three_sigils_go_to_a_page() {
    // `@sarah` and `^acme` were followable and `#reading` was a coloured span,
    // which is an inconsistency the reader has to learn rather than one they
    // can see. One page, from two tables, because the difference between an
    // OR-Set tag and a sigil in the text is not theirs to keep track of.
    let code = ui_code("app.js");
    assert!(
        code.contains(r##"openMentions("#""##),
        "a tag does not navigate anywhere"
    );
    assert!(
        code.contains(r#"call("tagged""#),
        "the tag page does not ask for tags added after the fact"
    );
    assert!(
        code.contains(r#"call("mentioning""#),
        "the page does not ask for sigils written in the text"
    );
}

#[test]
fn nothing_creates_an_entity() {
    // A page exists for every name the moment it is written, which is what
    // makes the layer usable on the first day rather than after months of
    // curation. A "new entity" control would be exactly the filing step §3.7
    // exists to remove — and would let a page exist that nothing mentions.
    let code = ui_code("app.js");
    for creating in ["new entity", "createEntity", "addPerson", "newProject"] {
        assert!(
            !code.contains(creating),
            "the interface can create an entity ({creating:?}), which is filing"
        );
    }
}

#[test]
fn review_keeps_no_score_of_any_kind() {
    // The surface where a graded record would do the most harm: recurring,
    // permanently recorded, delivered on a schedule the user did not set, to
    // somebody whose stated difficulty is memory. §2.7 removed the grade rather
    // than the feature, and the absence is a requirement rather than an
    // omission — which is exactly what a later change would fill in.
    //
    // Code only. This file argues the point in prose, and the first version of
    // the equivalent test in `review.rs` failed on its own comments.
    let code = ui_code("app.js");
    for forbidden in ["streak", "accuracy", "grade", "correct", "score"] {
        assert!(
            !code.contains(forbidden),
            "the review pane grew a {forbidden:?} — §2.7 says it must not"
        );
    }
}

#[test]
fn a_session_is_fetched_once_rather_than_on_every_visit() {
    // Asking for a session is a write: being offered is the outcome `shown`,
    // which moves `last_shown` and makes those notes ineligible for a day. A
    // pane that re-fetched on navigation would spend somebody's five notes
    // because they glanced at the timeline and came back.
    let code = ui_code("app.js");
    assert!(
        code.contains("if (!session)"),
        "the review pane fetches unconditionally, so navigating spends a session"
    );
}

#[test]
fn review_does_not_redraw_on_its_own() {
    // Two reasons, and the second is the one that bites: a redraw would discard
    // a session part-way through, and the `shown` events this pane writes are
    // themselves a store change — so a review that refreshed on change would
    // refresh because of itself.
    let code = ui_code("app.js");
    let refresh = code
        .split("function refresh()")
        .nth(1)
        .expect("refresh() exists");
    let body = &refresh[..refresh.find("\n}").unwrap_or(refresh.len())];
    assert!(
        !body.contains("showReview"),
        "refresh() redraws review, which discards a session in progress"
    );
    assert!(
        body.contains("showTimeline"),
        "this test would pass vacuously if refresh() drew nothing at all"
    );
}

#[test]
fn an_answered_item_stays_answered_across_a_visit() {
    // Found by using it: the answers lived in the DOM, so leaving the pane and
    // coming back redrew the buttons on items already dealt with. Pressing one
    // again writes a second event, and `deferred` accumulates — two deferrals
    // retire an item permanently. A trip through the nav bar could have quietly
    // ended a note the user pushed away exactly once.
    let code = ui_code("app.js");
    assert!(
        code.contains("item.answered") && code.contains("n.answered"),
        "answers are not recorded on the session, so a redraw offers them again"
    );
}

#[test]
fn leaving_a_review_is_not_the_same_as_deferring() {
    // Two deferrals end an item permanently. That is a real consequence, and
    // closing a window must not be able to trigger it — only the button may.
    let code = ui_code("app.js");
    let n = code.matches("\"deferred\"").count();
    assert_eq!(
        n, 1,
        "expected exactly one place to record a deferral (the button), found {n}"
    );
}

#[test]
fn a_store_error_is_not_a_broken_window() {
    // These need different words and for a while had the same ones. A
    // projection being rebuilt after an upgrade made the page announce that
    // something in it was broken — and the evidence it was wrong is that the
    // window fixed itself a few seconds later, which a broken one does not.
    let code = ui_code("app.js");
    assert!(
        code.contains("class StoreError"),
        "nothing distinguishes a store refusing from a fault in this page"
    );
    assert!(
        code.contains("instanceof StoreError"),
        "the rejection handler does not tell the two apart"
    );
    assert!(
        code.contains("function notReady"),
        "there is no way to report a store error that is not `fatal`"
    );
}

#[test]
fn the_page_talks_only_to_the_commands_it_is_given() {
    // No fetch, no URL, no server. The capability list is the invoke handler in
    // main.rs, and it is only a real boundary if the page has no other way out.
    let js = ui_code("app.js");
    for escape in ["fetch(", "XMLHttpRequest", "WebSocket", "import("] {
        assert!(!js.contains(escape), "the page reached for {escape}");
    }
}

#[test]
fn every_command_the_page_calls_is_registered() {
    // A typo in a command name is invisible until the button is pressed, and
    // then it fails at the worst moment — halfway through something the user
    // asked for.
    let js = ui("app.js");
    let main =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/main.rs"))
            .unwrap();

    let mut called: Vec<String> = Vec::new();
    for pattern in ["invoke(\"", "call(\""] {
        let mut rest = js.as_str();
        while let Some(i) = rest.find(pattern) {
            rest = &rest[i + pattern.len()..];
            if let Some(end) = rest.find('"') {
                called.push(rest[..end].to_string());
            }
        }
    }
    assert!(!called.is_empty(), "the page calls something");

    for name in called {
        assert!(
            main.contains(&format!("ipc::{name},")),
            "the page calls `{name}`, which is not in the invoke handler"
        );
    }
}
