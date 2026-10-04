# Tasks

## 1. Pin the gap first

- [ ] 1.1 A failing test: with a capture unread, `agenda --json` carries the
      count. Confirm it fails against today's bare array.

## 2. The shape

- [ ] 2.1 `agenda --json` answers `{"entries":[...],"unread":N}`.
- [ ] 2.2 The field is present at zero, so the shape does not depend on the
      state — a consumer that worked all week must not break the first morning
      the sweep falls behind.
- [ ] 2.3 Update the usage text if it describes the old shape.

## 3. Guards

- [ ] 3.1 Check whether anything in the repo parses `agenda --json` as an array
      — tests, tools, the desktop, the phone — and fix what does.
- [ ] 3.2 `make ci`.

## 4. The question this leaves

- [ ] 4.1 Record, without answering it here: the requirement names agenda,
      tasks **and occurrences**, and there is no `occurrences` command at all.
      Occurrences are reachable one day at a time through the agenda, and
      whether that satisfies "list ... occurrences" is a separate question.
