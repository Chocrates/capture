//! `capture-desktop` — the browse surface.
//!
//! A webview for the views, with the core linked directly. There is no server
//! and no port: inventing one would put a network boundary inside a single-user
//! desktop application, and every request would then be something that could
//! arrive from somewhere else.
//!
//! Everything the page can do is enumerated in `ipc.rs`. That is the whole
//! attack surface and the whole capability list, and it fits on a screen.

mod ipc;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    tauri::Builder::default()
        .manage(ipc::Watcher::default())
        .manage(ipc::Pairing::default())
        .invoke_handler(tauri::generate_handler![
            ipc::changed,
            ipc::timeline,
            ipc::agenda,
            ipc::tasks,
            ipc::set_occurrence,
            ipc::set_task_done,
            ipc::note,
            ipc::search,
            ipc::names,
            ipc::mentioning,
            ipc::list,
            ipc::tick_item_at,
            ipc::accept_suggestion,
            ipc::set_kind,
            ipc::as_task,
            ipc::fix_list,
            ipc::tagged,
            ipc::wiki_index,
            ipc::set_parent,
            ipc::rename_entity,
            ipc::merge_entities,
            ipc::bind_backing,
            ipc::ambient,
            ipc::filter_by_context,
            ipc::health,
            ipc::pending_redactions,
            ipc::edit,
            ipc::tag,
            ipc::untag,
            ipc::set_alias,
            ipc::pin,
            ipc::mute,
            ipc::resolve,
            ipc::redact,
            ipc::cancel_redaction,
            ipc::acknowledge_exports,
            ipc::review,
            ipc::reviewed,
            ipc::carries_forward,
            ipc::split_series,
            ipc::retire_device,
            ipc::start_pairing,
            ipc::pairing_state,
            ipc::tick,
        ])
        .run(tauri::generate_context!())
        .expect("the window starts");
}
