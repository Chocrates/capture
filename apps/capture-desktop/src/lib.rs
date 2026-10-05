//! The window's capabilities, as a library.
//!
//! # Why this exists
//!
//! Everything the page can do is enumerated in `ipc`. That is the whole attack
//! surface and the whole capability list, and until now it was reachable only
//! from `main.rs` — so nothing could test it. The only test over this crate
//! reads `app.js` and `index.html` as *text*, which checks that the page calls
//! a command and cannot check what the command does.
//!
//! What that cost, concretely: `ipc::agenda` built its day window in UTC, so
//! every evening appointment west of the meridian landed on the wrong day
//! while the terminal showed it correctly. It took a person comparing two
//! surfaces to notice. The store query underneath was right; the four lines
//! turning a date into an instant were not, and nothing could reach them.
//!
//! So the crate is a library with a thin binary on top. `main.rs` keeps the
//! window: the builder, the handler list, the tracing setup. Everything a test
//! might want to ask is here.
//!
//! # What a test can and cannot do with this
//!
//! A command reads the store named by `CAPTURE_HOME`, so a test sets that and
//! calls the function. What it does *not* get is the page: `#[tauri::command]`
//! generates a wrapper for the webview, and the annotated function underneath
//! is an ordinary one — calling it proves the command is right and proves
//! nothing about whether the page calls it. `tools/unreached.py` is what
//! checks the other half, and the two together are the whole boundary.

pub mod ipc;
