//! Checklist lines in a page: `- `, `* `, `[ ]`, `[x]`, and markdown's
//! `- [ ]`. One item per line, ticked by line (pages, 2026-10-13).

use capture_parse::checklist_of;

fn items(text: &str) -> Vec<(String, bool)> {
    checklist_of(text)
        .into_iter()
        .map(|l| (l.item, l.done))
        .collect()
}

#[test]
fn each_marked_line_is_an_item_in_order() {
    let page = "Packing\n\n- passport\n* charger\n[ ] adapter\n[x] tickets\n[X] visa\n- [ ] socks\n- [x] book\nnot an item";
    assert_eq!(
        items(page),
        vec![
            ("passport".into(), false),
            ("charger".into(), false),
            ("adapter".into(), false),
            ("tickets".into(), true),
            ("visa".into(), true),
            ("socks".into(), false),
            ("book".into(), true),
        ]
    );
}

#[test]
fn indentation_and_trailing_space_do_not_matter() {
    assert_eq!(
        items("  - milk  \n\t[ ] eggs"),
        vec![("milk".into(), false), ("eggs".into(), false)]
    );
}

#[test]
fn a_marker_with_nothing_after_it_is_not_an_item() {
    assert!(items("- \n[ ]\n-\n- [ ] ").is_empty());
}

#[test]
fn prose_that_merely_starts_with_a_dash_word_is_not_an_item() {
    // No space after the dash: a word, a number, a range.
    assert!(items("-5 degrees\n--verbose\n*emphasis*\n[link](x)").is_empty());
}

#[test]
fn a_heading_or_a_tag_is_not_an_item() {
    assert!(items("# Heading\n#tag at the start").is_empty());
}

#[test]
fn each_item_knows_which_line_it_was_written_on() {
    // So a surface can draw the tick box where the line is, instead of the
    // text and then the list again beneath it (pages, 2026-10-15).
    let lines: Vec<usize> = checklist_of("Packing\n- passport\nprose\n\n[x] charger")
        .into_iter()
        .map(|l| l.line)
        .collect();
    assert_eq!(lines, vec![1, 4]);
}
