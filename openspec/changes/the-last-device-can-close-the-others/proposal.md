## Why

`Archive::retire` exists on the binding and its doc says exactly why:

> Here because the phone may be the only device left. Retiring a lost phone
> from the desktop was always possible; a lost *desktop* left the phone holding
> the archive and unable to close anything, **which is the half of §5.3's
> remedy that was never reachable.**

Kotlin never calls it. Nor `readmission_request`, nor `cosign`. Of the 74
methods the binding exposes, those three and `note_count` are the only ones the
application does not reference — and the three are the whole of device
membership.

So the half of the remedy that was never reachable is still not reachable. The
method written to close that gap is as unreachable as the gap it was written to
close.

The phone already lists the devices. `Model.peers` is populated from
`archive.peers()` and the health screen draws each one with its capabilities,
when it was last reached, and a control for choosing where things are
announced. It shows you a device you no longer have and offers to send your
alerts to it.

Lose the desktop and this is the situation: the archive is intact on the phone,
the lost device's key is still a member, and the only surface that can revoke it
is the one on the device you lost.

## What Changes

- **A device can be retired from the phone**, through the binding method that
  exists for it, on the screen that already lists devices.
- **Behind a confirmation that names the device.** §5.3 makes retirement
  recoverable only by a two-signature readmission, which needs two devices —
  and somebody doing this has just lost one. A mis-tap is therefore close to
  unrecoverable, so the gesture is not a bare button.
- **This device is not offered.** Retiring the device you are holding would
  revoke the key of the only member left, which is the one state §5.3's
  readmission cannot be written from.
- **The archive decides, Kotlin chooses when to ask.** The prefix match, the
  ambiguity refusal and the event are all the binding's; the surface decides
  only where the control sits and what the confirmation says.

## Capabilities

### Modified Capabilities

- `browse`: that device membership can be closed from the phone.

## Impact

- `android/app/src/main/java/dev/capture/Model.kt` — the call and the reload.
- `android/app/src/main/java/dev/capture/Screens.kt` — the control and its
  confirmation.
- `android/app/src/main/java/dev/capture/MainActivity.kt` — wiring.

## Non-goals

- **Readmission from the phone.** `cosign` and `readmission_request` are also
  uncalled, and they are the *recovery* path: two devices signing, one of them
  the one being readmitted. That is a flow rather than a button, it needs a
  second device present, and bundling it would hold up the half that is one
  call. Recorded rather than left silent.
- **A device screen of its own.** The health screen already lists devices and
  is where somebody goes when something is wrong, which is when this is
  reached for.
- **Changing what retirement does.** §5.3's semantics, the prefix match and the
  refusal on an ambiguous prefix are the binding's and are not touched.
