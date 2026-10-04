## 1. Keeping it

- [x] 1.1 `Note.cancelled_too_late`, set in the branch that computed both halves and discarded their combination — the content gone, the governing order withdrawn.
- [x] 1.2 A `late_cancels` table, because the schema may add tables and never columns. `PROJECTION_SCHEMA` 11 → 12. Derived from the log, so the rebuild finds every past instance rather than only new ones.

## 2. Reporting it

- [x] 2.1 `capture status` reports it as wanting attention, naming the notes, and saying another device may have cancelled in time while this one did not.
- [x] 2.2 **No remedy offered**, because there is none. A snapshot may exist and that is a separate mechanism with its own window and its own command; pointing at `capture recover` here would imply the two are connected.

## 3. Seeing it

- [x] 3.1 A cancellation folded after the destruction is recorded against the note.
- [x] 3.2 A cancellation folded before it is not recorded as late, and the note survives — the guard that cancelling in time does not look like cancelling too late.
- [x] 3.3 A destruction nobody cancelled records nothing.
- [x] 3.4 The test writes the cancellation straight into the log, because `mutate::cancel_redaction` now refuses once the destruction has happened. That refusal is the *local* half, fixed separately; this is the half that arrives from a peer, which no local refusal can prevent.
- [x] 3.5 Non-vacuous, with the sabotage confirmed to have applied first.
