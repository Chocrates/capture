//! The list form, and the sentences it must not eat.
//!
//! `#groceries: milk, eggs, bread` is a list. The work is not recognising that
//! — it is declining everything else, because §3.1.5's rule is that a match
//! firing inside ordinary writing is worse than no match at all. A list form
//! that triggered on `#standup at 9:30` would turn a tagged note about a
//! meeting into a list named "standup at 9", and the person who typed it would
//! have no idea why.
//!
//! So the examples below are mostly negative, and the properties are mostly
//! about what the parser refuses to claim.

use capture_parse::list_of;
use proptest::prelude::*;

#[test]
fn the_form() {
    let list = list_of("#groceries: milk, eggs, bread").expect("a list");
    assert_eq!(list.tag, "groceries");
    assert_eq!(list.items, ["milk", "eggs", "bread"]);
}

#[test]
fn order_is_the_order_written() {
    let list = list_of("#groceries: eggs, milk").expect("a list");
    assert_eq!(
        list.items,
        ["eggs", "milk"],
        "display order is written order"
    );
}

#[test]
fn spacing_around_a_separator_does_not_matter() {
    let list = list_of("#groceries: milk ,eggs,  bread").expect("a list");
    assert_eq!(list.items, ["milk", "eggs", "bread"]);
}

#[test]
fn a_trailing_separator_names_nothing() {
    let list = list_of("#groceries: milk, eggs,").expect("a list");
    assert_eq!(list.items, ["milk", "eggs"]);
}

#[test]
fn an_empty_item_between_separators_is_discarded() {
    let list = list_of("#groceries: milk,, eggs").expect("a list");
    assert_eq!(list.items, ["milk", "eggs"]);
}

#[test]
fn leading_whitespace_is_invisible_and_so_allowed() {
    assert!(list_of("  #groceries: milk").is_some());
}

/// The guards. Each of these is a sentence somebody would actually write, and
/// each was the reason for one of the three rules in `list_of`.
#[test]
fn what_is_not_a_list() {
    for text in [
        // Not anchored: prose with a tag in it.
        "ask sarah about #groceries: she has the list",
        "reminder about #groceries: ask first",
        // The nearest miss there is — a clock has a colon in it.
        "#standup at 9:30",
        "#standup tomorrow at 14:00",
        // No sigil at all.
        "groceries: milk, eggs, bread",
        // A tag with nothing after the colon.
        "#groceries:",
        "#groceries:   ",
        // A sigil that names nothing. The untagged form is written without
        // one; reading this as a tag called "" would file things under
        // nothing at all.
        "#: milk",
        // No colon.
        "#groceries milk eggs",
        "#groceries",
        // Empty.
        "",
        " ",
    ] {
        assert!(
            list_of(text).is_none(),
            "should not be a list, but was: {text:?}",
        );
    }
}

/// Splitting on the *first* colon, not the last.
///
/// An item containing a colon is likelier than a tag containing one, so the
/// colon after the slug is the one that means "items follow".
#[test]
fn an_item_may_contain_a_colon() {
    let list = list_of("#groceries: milk: the good one, eggs").expect("a list");
    assert_eq!(list.tag, "groceries");
    assert_eq!(list.items, ["milk: the good one", "eggs"]);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    /// Whatever it claims, the claim is well formed.
    ///
    /// Stated over arbitrary text rather than over examples because the guards
    /// are where an anchored-match bug hides, and a bug there looks like a
    /// perfectly ordinary sentence quietly becoming a list. Examples only
    /// cover the sentences somebody thought to write down.
    #[test]
    fn a_recognised_list_is_always_well_formed(s in ".*") {
        if let Some(list) = list_of(&s) {
            // A tag is optional now: a checklist with no label is one errand
            // with parts rather than a thing to file. Where there *is* one it
            // is still a single slug.
            prop_assert!(
                !list.tag.chars().any(char::is_whitespace),
                "the tag is one slug: {:?}", list.tag,
            );
            prop_assert!(!list.items.is_empty(), "a list of nothing is not a list");
            for item in &list.items {
                prop_assert!(!item.is_empty(), "an empty item is never named");
                prop_assert_eq!(item.trim(), item.as_str(), "items arrive trimmed");
            }
            // Anchored, which is the guard that matters. Two openers now, and
            // neither may appear anywhere but the front.
            let front = s.trim_start();
            prop_assert!(
                front.starts_with('#') || front.starts_with(':'),
                "only an anchored opener starts a list: {:?}", s,
            );
        }
    }

    /// The same, weighted toward text shaped like a capture.
    #[test]
    fn nor_does_capture_shaped_text_become_a_list_by_accident(
        s in prop::collection::vec(
            prop_oneof![
                "[a-z]{1,8}",
                "[0-9]{1,2}:[0-9]{2}",
                "[#@>!~^][a-z0-9]{0,6}",
                "[.,;:'\"()-]",
            ],
            0..10,
        ).prop_map(|v| v.join(" "))
    ) {
        if let Some(list) = list_of(&s) {
            let front = s.trim_start();
            prop_assert!(front.starts_with('#') || front.starts_with(':'));
            prop_assert!(!list.items.is_empty());
        }
    }

    /// A list somebody wrote reads back as what they wrote.
    ///
    /// The generator excludes commas and colons inside items and leading or
    /// trailing whitespace, because those are the documented limits of the
    /// form rather than properties of it — an item cannot contain a separator
    /// and there is no escape.
    #[test]
    fn a_written_list_reads_back(
        tag in "[a-z][a-z0-9-]{0,10}",
        items in prop::collection::vec("[a-z][a-z0-9 ]{0,12}[a-z0-9]", 1..6),
    ) {
        let written = alloc_line(&tag, &items);
        let list = list_of(&written).expect("what was written is a list");
        prop_assert_eq!(&list.tag, &tag);
        prop_assert_eq!(list.items.len(), items.len());
        for (got, want) in list.items.iter().zip(items.iter()) {
            prop_assert_eq!(got, want);
        }
    }
}

fn alloc_line(tag: &str, items: &[String]) -> String {
    format!("#{tag}: {}", items.join(", "))
}

// --------------------------------------------------------- untagged --------
//
// A checklist without a label. Requiring a tag is what made every checklist a
// collection as well: a task with parts does not want filing under anything,
// and `: pack passport, charger, adapter` is one errand with three bits.

#[test]
fn a_checklist_needs_no_tag() {
    let list = list_of(": milk, eggs, bread").expect("a list");
    assert_eq!(list.tag, "", "an untagged list should name no tag");
    assert_eq!(list.items, ["milk", "eggs", "bread"]);
}

#[test]
fn an_untagged_checklist_is_anchored_just_as_hard() {
    // A colon mid-sentence is ordinary writing, and §3.1.5 rates a match firing
    // inside prose as worse than no match at all. These are the sentences that
    // would have been destroyed by a looser rule.
    for prose in [
        "ask sarah: she has it",
        "the plan: get it done",
        "note to self: milk, eggs",
        "standup at 9:30",
        "budget 40k: revisit in Q2",
    ] {
        assert!(
            list_of(prose).is_none(),
            "{prose:?} is a sentence, not a list"
        );
    }
}

#[test]
fn leading_space_is_still_invisible() {
    // Whitespace before it is invisible to whoever typed it; anything else is
    // not, which is the same rule the tagged form follows.
    assert!(list_of("   : milk, eggs").is_some());
    assert!(list_of("x : milk, eggs").is_none());
}

#[test]
fn a_colon_with_nothing_after_it_is_not_a_list() {
    assert!(list_of(":").is_none());
    assert!(list_of(":   ").is_none());
    assert!(list_of("#shop:").is_none());
}

#[test]
fn a_stray_sigil_before_the_colon_is_not_an_empty_tag() {
    // `#: milk` is the untagged form written with a sigil that names nothing.
    // Reading it as a tag called "" would file things under nothing at all.
    assert!(list_of("#: milk, eggs").is_none());
}
