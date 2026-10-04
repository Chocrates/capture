# Tasks

## 1. Pin the gap first

- [x] 1.1 A failing test: with a capture unread, `agenda --json` carries the
      count. Confirm it fails against today's bare array.

## 2. The shape

- [x] 2.1 `agenda --json` answers `{"entries":[...],"unread":N}`.
- [x] 2.2 The field is present at zero, so the shape does not depend on the
      state — a consumer that worked all week must not break the first morning
      the sweep falls behind.
- [x] 2.3 Update the usage text if it describes the old shape. It does not —
      the line reads "capture agenda [day]  what is due and what occurs,
      default today" and says nothing about JSON.

## 3. Guards

- [x] 3.1 Check whether anything in the repo parses `agenda --json` as an array
      — tests, tools, the desktop, the phone — and fix what does. **Nothing
      does.** No CLI agenda test existed at all, no tool reads it, and the
      desktop and phone call the store directly rather than shelling out. So
      the shape change costs nothing inside the repo; it is a break only for a
      script somebody wrote outside it.
- [x] 3.2 `make ci`. 78 suites green; `deny` fails on network only.

## 4. The question this leaves

- [x] 4.1 Record, without answering it here: the requirement names agenda,
      tasks **and occurrences**, and there is no `occurrences` command at all.
      Occurrences are reachable one day at a time through the agenda, and
      whether that satisfies "list ... occurrences" is a separate question.
      Recorded in the review ledger, unanswered.
