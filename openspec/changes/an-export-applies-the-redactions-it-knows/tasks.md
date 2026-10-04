## 1. The strip set

- [x] 1.1 The targets of non-cancelled queued redactions join the set whose bodies are emptied.
- [x] 1.2 From `redact_queue`, which the fold already maintains including the cancellation rule. Decoding every `RedactBody` in the log would be a second implementation of that rule, which is the shape this project has been paying for all week.
- [x] 1.3 Notes whose destruction has already run need no entry — their bodies are already `None`, so there is nothing to strip.

## 2. The report

- [x] 2.1 Counted and reported by reason, with no remedy offered for the redacted ones. A credential flag over-flags on purpose and may be a false positive to clear with `capture unflag`; a redaction is a decision nothing should undo, and one number for both would put that advice beside notes where it is wrong.

## 3. Seeing it

- [x] 3.1 Redact, export before the destruction runs — which is the interval that leaked — and the text is not in the file.
- [x] 3.2 A cancelled redaction exports whole.
- [x] 3.3 An ordinary note exports whole, which is the guard against stripping everything and satisfying 3.1 for the wrong reason.
- [x] 3.4 Driven through the binary rather than the library, because the leak was in the command somebody actually runs.
- [x] 3.5 **Verified non-vacuous, and the sabotage verified to have applied first** — the lesson from the ambient change, where an edit that silently matched nothing produced a false all-clear on a privacy fix.
