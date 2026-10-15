//! Pages: notes written to be read and revised, never read for a type.
//!
//! Asked for on 2026-10-13 as "the last big piece to make this usable". A page
//! is a note whose promotion is [`Promoted::Page`], written pinned — the one
//! mechanism the store already had for "no parse may touch this" (§6). So an
//! edit to a page changes its words and nothing else, the calendar and the task
//! list never see it, and `promote::reread` hands it back to the recognizer.
//!
//! Its checklist lines (`- `, `[ ]`, `[x]`) are expanded by the projection into
//! the same rows a `#list:` capture's items use, so ticking one is the item tick
//! every surface already draws.

use capture_core::body::Promoted;

use crate::session::Session;
use crate::{Error, Result, StorePaths};

/// A page just made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Created {
    /// Its permanent name.
    pub slug: String,
    /// The name taken from its title, where that was free. `None` when another
    /// note already had it: the page keeps its slug, and a surface says so.
    pub name: Option<String>,
}

/// Start a page with a title, optionally under another page.
///
/// The title is captured as the note's text, so the slug is made from it as
/// the frozen slug algorithm requires (from the root capture), and the page is
/// named by the title's slug form when nothing else holds that name.
pub fn create(paths: &StorePaths, title: &str, under: Option<&str>) -> Result<Created> {
    let title = title.trim();
    if title.is_empty() {
        return Err(Error::Usage(String::from("a page needs a title")));
    }
    if title.contains('\n') {
        return Err(Error::Usage(String::from(
            "a title is one line; the page's body is written after it",
        )));
    }

    let slug = {
        let mut s = Session::open_for_capture(paths.clone())?;
        let body = crate::session::capture_body(title)?;
        s.capture(&body)?.slug
    };
    make(paths, &slug)?;

    let wanted = name_for(title);
    let name = match wanted {
        Some(n) if crate::projection::current(paths)?.resolve(&n)?.is_none() => {
            crate::mutate::set_alias(paths, &slug, Some(&n))?;
            Some(n)
        }
        _ => None,
    };
    if let Some(parent) = under {
        crate::mutate::set_parent(paths, &slug, Some(parent))?;
    }
    Ok(Created { slug, name })
}

/// Start a page that *is* a person, project or tag: made as [`create`] makes
/// one, then bound as its backing note, so the entity's page shows it above
/// everything that names it.
///
/// The entity is asked for first, so a name nobody has written refuses before
/// a capture exists rather than leaving an orphan page behind.
pub fn create_for(paths: &StorePaths, kind: &str, name: &str, title: &str) -> Result<Created> {
    if crate::projection::current(paths)?
        .entity_id(kind, name)?
        .is_none()
    {
        return Err(Error::NotFound(format!(
            "no {kind} called '{name}' — an entity exists because a note names it"
        )));
    }
    let made = create(paths, title, None)?;
    crate::mutate::bind_backing(paths, kind, name, Some(&made.slug))?;
    Ok(made)
}

/// Make any note a page. Its words are untouched; only what it is changes.
pub fn make(paths: &StorePaths, name: &str) -> Result<()> {
    crate::promote::correct(paths, name, Promoted::Page)
}

/// `car-maintenance` for "Car maintenance": lowercase letters and digits,
/// everything else a single hyphen. `None` where nothing is left.
pub fn name_for(title: &str) -> Option<String> {
    let mut out = String::new();
    for c in title.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    (!out.is_empty()).then_some(out)
}

/// One thing an editor can offer after a sigil.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    /// What goes after the sigil: `car-maintenance`, `sarah`.
    pub insert: String,
}

/// What to offer after `[[`, `@`, `^` or `#`, beginning with `prefix`.
///
/// One read for both editors, so the phone and the desktop offer the same
/// names. Pages by name; people, projects and tags by how often they are used,
/// which is the order a person reaches for them.
pub fn completions(
    store: &crate::projection::Store,
    sigil: &str,
    prefix: &str,
    limit: usize,
    now: u64,
) -> Result<Vec<Completion>> {
    let names = match sigil {
        "[[" => store.names_like(prefix, limit)?,
        "@" | "^" | "#" => {
            let sigil = sigil.chars().next().expect("one character");
            let prefix = prefix.to_lowercase();
            store
                .distinct_mentions(sigil, now)?
                .into_iter()
                .map(|(value, _)| value)
                .filter(|v| v.to_lowercase().starts_with(&prefix))
                .take(limit)
                .collect()
        }
        other => {
            return Err(Error::Usage(format!(
                "nothing completes after '{other}'; use [[, @, ^ or #"
            )));
        }
    };
    Ok(names
        .into_iter()
        .map(|insert| Completion { insert })
        .collect())
}
