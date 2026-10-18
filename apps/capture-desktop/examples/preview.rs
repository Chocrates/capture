//! The window's pages, served to an ordinary browser against a scratch store.
//!
//! The real window cannot be screenshotted on a GNOME/Wayland machine, and a
//! page nobody has looked at can be wrong in ways no test notices: the note
//! page read `page.items` where the items are `page.note.items` and drew no
//! tick boxes for months (found 2026-10-16). This serves `ui/` with a stand-in
//! for `window.__TAURI__` whose `invoke` calls the same `ipc` functions the
//! window does, so headless Chrome can render and photograph any view.
//!
//! Read-only commands only; anything else answers with an error the page will
//! show. Refuses to start without `CAPTURE_HOME`, so it cannot be pointed at a
//! real archive by accident.
//!
//! ```sh
//! CAPTURE_HOME=/tmp/scratch cargo run -p capture-desktop --example preview -- 8765
//! google-chrome --headless=new --screenshot=out.png 'http://127.0.0.1:8765/#note=trip'
//! ```
//!
//! The fragment picks the view: `#timeline`, `#agenda`, `#tasks`, `#wiki`,
//! `#lists`, `#review`, `#health`, `#note=<name>`, `#search=<words>`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;

use capture_desktop::ipc;
use serde_json::{Value, json};

const UI: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/ui");

/// Stands in for Tauri, and opens the view the fragment names.
const STUB: &str = r#"<script>
window.__TAURI__ = { core: { invoke: async (name, args) => {
  const r = await fetch("/call/" + name, { method: "POST", body: JSON.stringify(args || {}) });
  const body = await r.json();
  if (!r.ok) throw body.error;
  return body.ok;
} } };
window.addEventListener("load", () => setTimeout(() => {
  const [view, arg] = decodeURIComponent(location.hash.slice(1)).split("=");
  if (view === "note") return showNote(arg);
  if (view === "search") { const q = document.getElementById("q"); q.value = arg; q.dispatchEvent(new Event("input")); return; }
  const b = document.querySelector(`nav button[data-view="${view}"]`);
  if (b) b.click();
}, 200));
</script>
"#;

fn main() {
    if std::env::var_os("CAPTURE_HOME").is_none() {
        eprintln!("preview: set CAPTURE_HOME to a scratch store; it never opens the real one");
        std::process::exit(2);
    }
    let port = std::env::args().nth(1).unwrap_or_else(|| "8765".into());
    let listener = TcpListener::bind(format!("127.0.0.1:{port}")).expect("bind");
    eprintln!("preview: http://127.0.0.1:{port}/");
    for stream in listener.incoming().flatten() {
        let mut stream = stream;
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            continue;
        }
        let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
        let mut length = 0usize;
        loop {
            let mut h = String::new();
            if reader.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
                break;
            }
            if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                length = v.trim().parse().unwrap_or(0);
            }
        }
        let mut body = vec![0; length];
        let _ = reader.read_exact(&mut body);

        let (status, kind, out) = if let Some(name) = path.strip_prefix("/call/") {
            let args: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
            let (status, v) = match call(name, &args) {
                Ok(v) => ("200 OK", json!({ "ok": v })),
                Err(e) => ("500 Internal Server Error", json!({ "error": e })),
            };
            (status, "application/json", v.to_string().into_bytes())
        } else {
            let file = match path.split(['?', '#']).next().unwrap_or("/") {
                "/" => "index.html",
                p => p.trim_start_matches('/'),
            };
            match std::fs::read(format!("{UI}/{file}")) {
                Ok(bytes) if file == "index.html" => {
                    let html = String::from_utf8_lossy(&bytes);
                    let at = html.find("<script").unwrap_or(html.len());
                    let page = format!("{}{STUB}{}", &html[..at], &html[at..]);
                    ("200 OK", "text/html", page.into_bytes())
                }
                Ok(bytes) => {
                    let kind = if file.ends_with(".css") {
                        "text/css"
                    } else {
                        "text/javascript"
                    };
                    ("200 OK", kind, bytes)
                }
                Err(_) => ("404 Not Found", "text/plain", b"not found".to_vec()),
            }
        };
        let _ = write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            out.len()
        );
        let _ = stream.write_all(&out);
    }
}

fn s(args: &Value, key: &str) -> String {
    args[key].as_str().unwrap_or_default().to_string()
}

fn ser<T: serde::Serialize>(r: Result<T, String>) -> Result<Value, String> {
    r.map(|v| serde_json::to_value(v).unwrap_or(Value::Null))
}

fn call(name: &str, a: &Value) -> Result<Value, String> {
    match name {
        // A scratch store nothing else writes to has not changed.
        "changed" => Ok(json!(false)),
        "timeline" => ser(ipc::timeline(a["limit"].as_u64().unwrap_or(100) as usize)),
        "agenda" => ser(ipc::agenda(s(a, "day"))),
        "month" => ser(ipc::month(s(a, "day"))),
        "tasks" => ser(ipc::tasks()),
        "note" => ser(ipc::note(s(a, "name"))),
        "search" => ser(ipc::search(s(a, "query"))),
        "mentioning" => ser(ipc::mentioning(s(a, "sigil"), s(a, "value"))),
        "list" => ser(ipc::list(s(a, "tag"))),
        "tagged" => ser(ipc::tagged(s(a, "tag"))),
        "wiki_index" => ser(ipc::wiki_index()),
        "health" => ser(ipc::health()),
        "pending_redactions" => ser(ipc::pending_redactions()),
        "review" => ser(ipc::review()),
        "ambient" => ser(ipc::ambient(s(a, "name"))),
        "completions" => ser(ipc::completions(s(a, "sigil"), s(a, "prefix"))),
        other => Err(format!("preview: `{other}` is not served")),
    }
}
