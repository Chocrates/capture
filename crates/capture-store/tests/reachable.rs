//! Every operation the store offers has a way to ask for it.
//!
//! Not a style check. Five functions here had no caller anywhere — and the one
//! that mattered, `is_stale`, meant a projection was never rebuilt after a
//! schema bump while the design assumed rebuild-not-migrate throughout. The
//! others were live in one surface and dead in the store, which is how the
//! note-scoped mute lost the `tgt` that puts it in `capture log`.
//!
//! An unreachable operation is worse than a missing one: it is documented,
//! tested in isolation, and reported as present.

/// Every *shipped* source file, as one string — `src` only, never `tests`.
///
/// The exclusion is the whole test. The first version scanned the workspace
/// including this file, so every name in the list below appeared at least twice
/// — once here and once at its definition — and the assertion held whether or
/// not anything could reach the operation. A test that cannot fail is worse
/// than no test, because it is reported as coverage.
///
/// A call from a test is also not reachability. It proves the function works,
/// not that anybody can ask for it.
fn all_source() -> String {
    fn walk(dir: &std::path::Path, out: &mut String) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name != "target" && name != "tests" && !name.starts_with('.') {
                    walk(&p, out);
                }
            } else if p.extension().and_then(|x| x.to_str()) == Some("rs")
                && let Ok(s) = std::fs::read_to_string(&p)
            {
                out.push_str(&s);
                out.push('\n');
            }
        }
    }

    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("the workspace root")
        .to_path_buf();

    let mut out = String::new();
    walk(&root.join("crates"), &mut out);
    walk(&root.join("apps"), &mut out);
    out
}

#[test]
fn every_mutation_the_store_offers_can_be_asked_for() {
    // Each of these writes to the log, and each is therefore a thing somebody
    // must be able to do. A mutation nothing calls is a promise with no path to
    // it — and the copy that *is* reachable, living somewhere else, drifts.
    let source = all_source();

    for op in [
        "mute_note",
        "mute_scope",
        "set_setting",
        "add_intent",
        "retract_intent",
        "cancel_redactions_since",
        "set_parent",
        "as_task",
        "set_done",
        "carries_forward",
        "set_alias",
    ] {
        let calls = source.matches(op).count();
        // Its definition, and at least one place that is not the definition.
        assert!(
            calls > 1,
            "nothing calls `{op}` — an operation nobody can reach is a promise with no path to it"
        );
    }

    // Non-vacuity, stated rather than assumed: a name that exists nowhere must
    // fail the same assertion, or the test above is counting its own list.
    assert_eq!(
        source.matches("mute_the_whole_archive").count(),
        0,
        "the scan is picking up this file, so the assertions above cannot fail"
    );
}
