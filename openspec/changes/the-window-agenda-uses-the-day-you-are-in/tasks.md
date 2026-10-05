# Tasks

## 1. The zone

- [x] 1.1 Resolve the system zone once at the top of `ipc::agenda`, falling back
      to UTC the way the terminal does — UTC as the fallback, not the answer.
- [x] 1.2 Use it to build the day's window.
- [x] 1.3 Use the same one for the "is this today" comparison that decides
      whether overdue work is shown.

## 2. Show it was wrong, and is not any more

- [x] 2.1 Record the demonstration in the change: an evening appointment, its
      stored instant, the window the old code built, and the gap between them.

      On a scratch store in `America/Denver`, `dinner 2026-10-05 19:00`:

      ```
      stored instant   1791248400000  =  2026-10-06 01:00 UTC
      window before    1791158400000 .. 1791244800000   (UTC midnight)   inside? no
      window now       1791180000000 .. 1791266400000   (local midnight) inside? yes
      ```

      An hour past the end of the old window. `capture agenda 2026-10-05` showed
      the dinner throughout; the window would have shown an empty evening.
- [x] 2.2 Re-run it after the fix against the terminal, which reads the same
      store with the right zone, and confirm they now agree. They do: the
      instant now falls inside the window the surface builds, which is the one
      the terminal has always used.

## 3. Guards

- [x] 3.1 `make ci`. 79 suites green; `deny` fails on network only.
- [x] 3.2 Check whether anything else in `ipc.rs` builds a zone the same way.
      One other site constructs UTC — `ipc::tasks`, where it is the *fallback*
      after the system zone, which is correct. The agenda was the only one using
      it as the answer.

## 4. The limit, stated

- [x] 4.1 Record that nothing will catch this coming back: the desktop is a
      binary crate, so `ipc` cannot be tested below the page. Name what would
      change that, without doing it here.

      A library target for `capture-desktop` would make `ipc` reachable from an
      integration test. Worth weighing: the window gained four commands this
      session — `list`'s slug, `tasks`, `set_kind`, `as_task` — and not one of
      them can be tested below `frontend.rs`, which reads the page as text.
