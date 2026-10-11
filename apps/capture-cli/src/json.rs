//! The `--json` renderings, as values rather than format strings.
//!
//! # Written by hand rather than derived
//!
//! These structures cross into scripts, and a serialiser attached to the
//! storage types would make every future field rename a silent breaking change
//! to someone's pipeline. So each command says, in its own source, every key it
//! emits.
//!
//! # One value per answer, two renderings of it
//!
//! Each read command builds one *view* and renders it twice. The prose renderer
//! takes the view and no store, so it can only print what the view holds; the
//! JSON renderer destructures the view without `..`, so a field added to it does
//! not compile until the JSON names it. Between them, anything a person can read
//! in a terminal a script can read here.
//!
//! The JSON path fell behind the prose four times before this — `tasks` without
//! its band, `agenda` without its unread count and then without an
//! occurrence's own title — and an audit after the fourth found six more. Each
//! was the same mistake: two pieces of code answering one question, and the one
//! nobody looks at while writing the command was the one left behind.

/// One command's answer, renderable both ways.
///
/// `prose` takes `&self` and nothing else on purpose: with no store in reach,
/// the only facts it can print are the ones the view holds — which are the ones
/// `json` is obliged to name.
pub trait View {
    /// The answer for a script.
    fn json(&self) -> Json;
    /// The answer for a person.
    fn prose(&self);
}

/// Print an answer in whichever form was asked for.
pub fn answer(v: &impl View, json: bool) {
    if json {
        say!("{}", v.json().render());
    } else {
        v.prose();
    }
}

/// A JSON value, with object keys in the order they were given.
///
/// Keys are `&'static str` on purpose. A key is part of the interface and
/// belongs in source where it can be read, never computed.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(&'static str, Json)>),
}

/// An object, from its fields in order.
pub fn obj<const N: usize>(fields: [(&'static str, Json); N]) -> Json {
    Json::Obj(fields.into())
}

/// An array, from anything that renders.
pub fn arr<T: Into<Json>>(items: impl IntoIterator<Item = T>) -> Json {
    Json::Arr(items.into_iter().map(Into::into).collect())
}

impl Json {
    /// The same object with more fields on the end.
    ///
    /// For a page that is a note *and* more — `show` is a note's fields
    /// followed by what it was recognised as and what it links to — so the
    /// note's own keys come from one place wherever a note is rendered.
    pub fn and<const N: usize>(mut self, more: [(&'static str, Json); N]) -> Json {
        match &mut self {
            Json::Obj(fields) => fields.extend(more),
            other => unreachable!("only an object takes fields, not {other:?}"),
        }
        self
    }

    /// The value as one line of JSON.
    pub fn render(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Int(n) => out.push_str(&n.to_string()),
            Json::Str(s) => out.push_str(&quote(s)),
            Json::Arr(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write(out);
                }
                out.push(']');
            }
            Json::Obj(fields) => {
                out.push('{');
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&quote(k));
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

impl From<bool> for Json {
    fn from(b: bool) -> Json {
        Json::Bool(b)
    }
}

impl From<&str> for Json {
    fn from(s: &str) -> Json {
        Json::Str(s.to_string())
    }
}

impl From<String> for Json {
    fn from(s: String) -> Json {
        Json::Str(s)
    }
}

impl From<&String> for Json {
    fn from(s: &String) -> Json {
        Json::Str(s.clone())
    }
}

/// Integers, through `i64`.
///
/// A `u64` above `i64::MAX` saturates rather than wrapping. Nothing here
/// counts that high — the largest numbers are millisecond timestamps — and a
/// negative timestamp would be a worse answer than a clamped one.
macro_rules! int {
    ($($t:ty),*) => {$(
        impl From<$t> for Json {
            fn from(n: $t) -> Json {
                Json::Int(i64::try_from(n).unwrap_or(i64::MAX))
            }
        }
        impl From<&$t> for Json {
            fn from(n: &$t) -> Json {
                Json::from(*n)
            }
        }
    )*};
}
int!(i64, u64, u32, u16, u8, usize);

impl<T: Into<Json>> From<Option<T>> for Json {
    fn from(v: Option<T>) -> Json {
        v.map_or(Json::Null, Into::into)
    }
}

impl From<&Option<String>> for Json {
    fn from(v: &Option<String>) -> Json {
        v.as_deref().into()
    }
}

/// A string as a JSON string literal.
///
/// Capture is verbatim, so a note may hold anything, and every byte below
/// `0x20` has to be escaped or a consumer will refuse the output.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_strings_are_escaped() {
        assert_eq!(quote("plain"), "\"plain\"");
        assert_eq!(quote("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(quote("a\nb"), "\"a\\nb\"");
        assert_eq!(quote("back\\slash"), "\"back\\\\slash\"");
    }

    #[test]
    fn control_characters_do_not_escape_the_string() {
        // Capture is verbatim, so a note may contain anything at all. A raw
        // control byte in the output would produce JSON a consumer rejects.
        let out = quote("bell\u{7}here");
        assert!(out.contains("\\u0007"), "got {out}");
    }

    #[test]
    fn an_object_keeps_the_order_it_was_given() {
        // `tools/converge.sh` reads the slug as the first field of a note, so
        // order is part of the interface rather than a cosmetic.
        let v = obj([
            ("slug", "a".into()),
            ("alias", Json::Null),
            ("n", 3u64.into()),
        ]);
        assert_eq!(v.render(), r#"{"slug":"a","alias":null,"n":3}"#);
    }

    #[test]
    fn nothing_renders_as_null_and_something_as_itself() {
        assert_eq!(Json::from(None::<&str>).render(), "null");
        assert_eq!(Json::from(Some("x")).render(), "\"x\"");
        assert_eq!(arr(["a", "b"]).render(), r#"["a","b"]"#);
        assert_eq!(arr(Vec::<Json>::new()).render(), "[]");
    }

    #[test]
    fn a_number_too_large_saturates_rather_than_going_negative() {
        assert_eq!(Json::from(u64::MAX).render(), i64::MAX.to_string());
    }
}
