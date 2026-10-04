## Context

The binding exposes 74 methods on `Archive`. The application references 70.
The four it does not are `note_count` and the whole of device membership:
`retire`, `readmission_request`, `cosign`.

`retire`'s own doc names the gap it was written for — a lost desktop leaving
the phone unable to close anything — and the gap is still open because nothing
calls it.

The phone already has the list: `Model.peers` from `archive.peers()`, drawn on
the health screen with capabilities, last-reached and a notifier control.

## Goals / Non-Goals

**Goals.** Retire another device from the phone. Ask first, naming it. Keep the
decision in the archive.

**Non-Goals.** Readmission from the phone. A device screen. Changing what
retirement means.

## Decisions

### On the health screen, not a new one

It is already the list of devices, and it is where somebody goes when something
is wrong — which is the state this is reached for. A screen of its own would be
a second place devices appear, and the one that does not get updated.

### A confirmation, because the undo needs two devices

§5.3: retirement is undone by a readmission signed by two devices, one of them
the device being readmitted. Somebody reaching for this has just lost a device,
so the undo may be unavailable at exactly the moment the mistake is made.

That is a different argument from §1.1's redaction prompt — nothing is
destroyed here — and it lands in the same place: not a bare button. The
confirmation names the device, because a count is dismissible in a way a name
is not.

*Rejected:* a type-to-confirm, as redaction uses. The key is a hex prefix
nobody can type from memory, and a confirmation somebody cannot complete is one
they work around.

### This device is not offered

Retiring the key you are holding leaves no member able to author a readmission,
which is the one state §5.3's remedy cannot reach. The list already knows which
row is this device — it draws "Notify me here" for it.

### Kotlin chooses when to ask and nothing else

The prefix match, the ambiguity refusal, the event and its signature are the
binding's. The surface decides where the control sits and what the
confirmation says, which is what `make android-decides` permits.

## Risks / Trade-offs

- **Retirement is reachable from a pocket.** → That is the point, and the
  confirmation is the answer. The alternative is the current state, where a
  lost desktop cannot be closed at all.
- **The undo is still not reachable from the phone.** → Named as a non-goal and
  recorded. `cosign` and `readmission_request` need two devices present, which
  is a flow rather than a button.

## Migration Plan

None. No event kind, no schema, no projection.

## Open Questions

- **Should the phone be able to readmit?** It is the other half, it is two
  uncalled methods, and it needs a second device in the room. Worth doing
  deliberately rather than as an afterthought to this.
