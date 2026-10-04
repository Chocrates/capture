## 1. The call

- [x] 1.1 `Model.retire(key)` calls the binding and re-reads the device list, the conditions and the notifier — a retired device must stop being offered as somewhere to send alerts.
- [x] 1.2 The archive's own refusal surfaces: the prefix match and the ambiguity refusal are its, and `load` carries the message it gives.

## 2. The control

- [x] 2.1 A retire control on each peer row of the health screen, where the devices already are.
- [x] 2.2 Never on this device's row. Revoking the key you are holding leaves no member able to author the readmission that would undo it, which is the one state §5.3's remedy cannot be written from.
- [x] 2.3 A confirmation naming the device, and naming the undo: a readmission signed by two devices, which somebody who has just lost one may not be able to produce. Not a type-to-confirm — the key is a hex prefix nobody can type from memory, and a guard nobody can clear is one that gets worked around rather than a stronger guard.

## 3. Checking it

- [x] 3.1 `make android-decides` clean: 22 Kotlin files, none deciding anything. The surface decides where the control sits and what the confirmation says; everything else is the binding's.
- [x] 3.2 Bindings regenerated, `make android-app` builds, `./gradlew assembleDebug` succeeds, and the APK installed on the device.
- [x] 3.3 **Confirmed on the device, 2026-10-04.** Against `dev.capture.dev`, paired to a *scratch* desktop store — never the real archive — because the control only exists where a peer does, and the phone had none.
      - The control appears on the peer's row and **not** on this phone's, which is the guard that matters.
      - The confirmation names the device and says the undo needs two devices to sign.
      - Declining leaves the peer listed and the control offered.
      - Retiring removes it: the list goes back to "No other devices yet."

      All five scenarios of the spec, on real hardware. Screenshot kept at
      `~/.local/share/capture-reviews/2026-10-04-retire-confirmation.png`.
