//! Opening database connections, with the settings that must never vary.
//!
//! # Why this is a module and not two lines at each call site
//!
//! Two SQLite settings matter here and they behave differently, which is
//! exactly the kind of asymmetry that produces a guarantee that holds in one
//! binary and quietly does not in another:
//!
//! - **`journal_mode = WAL`** is stored *in the database file*. Set it once and
//!   every future connection inherits it.
//! - **`synchronous = FULL`** is a property of *the connection*. It is not
//!   stored anywhere. A binary that forgets it gets a faster, less durable
//!   connection to the same file, and nothing reports the difference.
//!
//! Three processes will open this store — the overlay, the command line, and
//! the desktop application. If durability were configured at the point of
//! writing rather than the point of opening, "a capture the interface accepted
//! is never a lie" would be true of whichever binary the author had in mind.
//!
//! So every connection is opened through here.

use std::path::Path;

use rusqlite::Connection;

use crate::Result;

/// How long to wait for another process holding the write lock.
///
/// Several processes share this store, so contention is expected rather than
/// exceptional; without a timeout the loser fails immediately instead of
/// waiting a few milliseconds for a transaction that is already committing.
const BUSY_TIMEOUT_MS: u32 = 5_000;

/// Open a connection for writing.
pub fn open_write(path: &Path) -> Result<Connection> {
    let c = Connection::open(path)?;
    apply(&c, true)?;
    Ok(c)
}

/// Open a connection for reading only.
///
/// Read paths do not go through the writer. Diagnostics that stop working when
/// the system is unhealthy are unusable at the one moment they are needed, and
/// integrity checking in particular exists for exactly that moment.
pub fn open_read(path: &Path) -> Result<Connection> {
    let c = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    apply(&c, false)?;
    Ok(c)
}

/// Create a schema, all of it or none of it.
///
/// `execute_batch` runs each statement in its own implicit transaction, so a
/// process killed partway through leaves a database holding some of its tables.
/// That state is worse than no database at all: it opens, it answers questions
/// about the tables that exist, and it reports the missing ones as "no such
/// table" — which reads like a bug in the query rather than a half-built store.
/// A read-only opener cannot even repair it, because repairing means creating
/// tables.
///
/// The crash harness found this by killing a victim during its first
/// milliseconds, which is exactly when a real first run is most likely to be
/// interrupted.
pub fn create_schema(c: &Connection, schema: &str) -> Result<()> {
    c.execute_batch(&format!("BEGIN IMMEDIATE;\n{schema}\nCOMMIT;"))
        .inspect_err(|_| {
            let _ = c.execute_batch("ROLLBACK");
        })?;
    Ok(())
}

fn apply(c: &Connection, writable: bool) -> Result<()> {
    c.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS as u64))?;
    if writable {
        // Persisted in the file; setting it repeatedly is harmless.
        c.pragma_update(None, "journal_mode", "WAL")?;
        // Per connection, and therefore the one that must be set every time.
        c.pragma_update(None, "synchronous", "FULL")?;
        // Overwrite freed pages rather than leaving their contents on disk.
        // Redaction depends on this: without it, destroyed payloads persist in
        // pages the database considers free.
        c.pragma_update(None, "secure_delete", "ON")?;
    }
    c.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("brain-db-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.join("t.db")
    }

    fn pragma(c: &Connection, name: &str) -> String {
        c.query_row(&format!("PRAGMA {name}"), [], |r| {
            r.get::<_, rusqlite::types::Value>(0)
        })
        .map(|v| format!("{v:?}"))
        .unwrap_or_default()
    }

    #[test]
    fn a_writable_connection_is_durable() {
        let p = tmp("durable");
        let c = open_write(&p).unwrap();
        // 2 is FULL. The value that is not stored in the file, and so must be
        // set by every process that writes.
        assert!(pragma(&c, "synchronous").contains('2'));
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn wal_persists_but_synchronous_does_not() {
        // The asymmetry this module exists for, demonstrated with a value that
        // is definitely not the default — otherwise the test cannot tell
        // "we set it" from "it was already that".
        let p = tmp("asymmetry");
        {
            let c = open_write(&p).unwrap();
            c.pragma_update(None, "synchronous", "OFF").unwrap();
            assert!(pragma(&c, "synchronous").contains('0'));
        }
        let bare = Connection::open(&p).unwrap();
        assert!(
            pragma(&bare, "journal_mode").to_lowercase().contains("wal"),
            "journal mode is stored in the file and survives"
        );
        assert!(
            !pragma(&bare, "synchronous").contains('0'),
            "durability is per connection: the previous setting did not survive, \
             so a binary that never sets it gets whatever this build defaults to"
        );
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn freed_pages_are_overwritten() {
        // Redaction relies on this: otherwise a destroyed payload survives in
        // pages the database merely considers free.
        let p = tmp("secure");
        let c = open_write(&p).unwrap();
        assert!(pragma(&c, "secure_delete").contains('1'));
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn a_schema_is_created_whole_or_not_at_all() {
        // A store killed partway through its first run must not come back
        // holding half its tables: it would open, answer some questions, and
        // report the rest as "no such table" — indistinguishable from a bug in
        // the query.
        let p = tmp("atomic-schema");
        let c = open_write(&p).unwrap();
        let bad = "CREATE TABLE a (x INTEGER) STRICT;\nCREATE TABLE b (nonsense;";
        assert!(create_schema(&c, bad).is_err());
        assert!(
            pragma(&c, "user_version").len() < 100,
            "connection is still usable after the failure"
        );
        let exists: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'a'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(exists, 0, "the table that did parse must not survive alone");
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn a_read_connection_cannot_write() {
        let p = tmp("ro");
        {
            let c = open_write(&p).unwrap();
            c.execute_batch("CREATE TABLE t (x INTEGER)").unwrap();
        }
        let r = open_read(&p).unwrap();
        assert!(r.execute("INSERT INTO t VALUES (1)", []).is_err());
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn reading_does_not_need_the_writer() {
        let p = tmp("ro-independent");
        {
            let c = open_write(&p).unwrap();
            c.execute_batch("CREATE TABLE t (x INTEGER); INSERT INTO t VALUES (7)")
                .unwrap();
        }
        // No writer open at all.
        let r = open_read(&p).unwrap();
        let v: i64 = r
            .query_row("SELECT x FROM t", [], |row| row.get(0))
            .unwrap();
        assert_eq!(v, 7);
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }
}
