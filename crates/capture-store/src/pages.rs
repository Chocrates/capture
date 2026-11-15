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
    /// note already had it, or the title looked like a credential: the page
    /// keeps its slug, and a surface says why ([`Created::notice`]).
    pub name: Option<String>,
    /// Why the title gave no name, when it did not.
    pub unnamed: Option<Unnamed>,
}

/// Why a new page was not named by its title.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unnamed {
    /// Another note already answers to that name.
    Taken,
    /// The title looked like it holds a credential. Its slug is a date for the
    /// same reason (§4.3), and an alias made from the title would carry the
    /// words into every `[[link]]` that names the page.
    Credential,
}

impl Created {
    /// What a surface says when the page did not get its title's name — one
    /// wording for every surface. Never repeats the title.
    pub fn notice(&self) -> Option<String> {
        self.unnamed.map(|why| match why {
            Unnamed::Taken => format!("That name was taken, so this page is called {}.", self.slug),
            Unnamed::Credential => format!(
                "The title looks like it holds a credential, so this page is called {} rather than by its title.",
                self.slug
            ),
        })
    }
}

/// Start a page with a title, optionally under another page.
///
/// The title is captured as the note's text, so the slug is made from it as
/// the frozen slug algorithm requires (from the root capture), and the page is
/// named by the title's slug form when nothing else holds that name.
pub fn create(paths: &StorePaths, title: &str, under: Option<&str>) -> Result<Created> {
    create_with(paths, title, "", under)
}

/// Start a page with its body already written: one capture holding the title
/// line and the body under it, rather than an empty page and an edit. The
/// edit route also lost the title, because an edit replaces the whole text.
pub fn create_with(
    paths: &StorePaths,
    title: &str,
    body: &str,
    under: Option<&str>,
) -> Result<Created> {
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
        let body = body.trim_matches('\n');
        // A body that starts by repeating the title would show it twice: every
        // page a model wrote did this, reading "the first line is the title"
        // as an instruction to write it (2026-11-15).
        let body = match body.split_once('\n') {
            Some((first, rest)) if first.trim() == title => rest.trim_start_matches('\n'),
            None if body.trim() == title => "",
            _ => body,
        };
        let text = if body.trim().is_empty() {
            title.to_string()
        } else {
            format!("{title}\n\n{body}")
        };
        let body = crate::session::capture_body(&text)?;
        s.capture(&body)?.slug
    };
    make(paths, &slug)?;

    let (name, unnamed) = if capture_core::slug::looks_like_credential(title) {
        (None, Some(Unnamed::Credential))
    } else {
        match name_for(title) {
            Some(n) if crate::projection::current(paths)?.resolve(&n)?.is_none() => {
                crate::mutate::set_alias(paths, &slug, Some(&n))?;
                (Some(n), None)
            }
            Some(_) => (None, Some(Unnamed::Taken)),
            // Nothing nameable in the title (emoji only): the slug is its
            // name, which is no loss worth announcing.
            None => (None, None),
        }
    };
    if let Some(parent) = under {
        crate::mutate::set_parent(paths, &slug, Some(parent))?;
    }
    Ok(Created {
        slug,
        name,
        unnamed,
    })
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

/// The name a title gives a page. Lives in `capture_core::link`, because the
/// fold applies the same rule to the words inside a `[[link]]`.
pub use capture_core::link::name_for;

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
