#!/bin/sh
# Run the store's own test suite on a connected Android device.
#
# The point of this is not coverage — the suite already runs on Linux. It is
# that `capture-store` is full of assumptions that are true of Linux and merely
# *probably* true of Android: SQLite's WAL, `flock` on the writer lock, the
# spool's preallocation, `TimeZone::system()`, and what a temp directory is.
# Every one of those is cheap to check here and expensive to discover through a
# JNI stack trace after the Kotlin layer exists.
#
# It builds nothing that is not already built. Run `make android-build` first.

set -eu

DEVICE_DIR=/data/local/tmp/capture
TARGET_DIR=target/x86_64-linux-android/debug/deps

# Suites that cannot run on a device, and why.
#
# Neither is an Android problem, and both would otherwise look like one.
#
#   reachable — scans the *source tree* to check every store mutation has a
#               caller. There is no source tree on a phone.
#   crash     — spawns a sibling binary through `CARGO_BIN_EXE_crash-victim`,
#               a path that exists only where cargo built it.
#
# Listed rather than silently skipped: a suite that does not run is a claim
# nobody is making, and it should be visible which claims those are.
NOT_ON_DEVICE="reachable crash"

adb() { "${ANDROID_HOME:?run under make android}/platform-tools/adb" "$@"; }

adb wait-for-device
[ "$(adb shell getprop sys.boot_completed | tr -d '\r')" = "1" ] || {
    echo "the device is connected but has not finished booting"
    exit 1
}
echo "device: $(adb shell getprop ro.product.cpu.abi | tr -d '\r')"

adb shell mkdir -p "$DEVICE_DIR" >/dev/null 2>&1 || true

fail=0
ran=0
for binary in "$TARGET_DIR"/*; do
    case "$binary" in
        *.d | *.rlib | *.rmeta | *.so) continue ;;
    esac
    [ -x "$binary" ] || continue

    name=$(basename "$binary" | sed 's/-[0-9a-f]*$//')
    case " $NOT_ON_DEVICE " in
        *" $name "*)
            echo "  $name: not run on a device — see tools/android-test.sh"
            continue
            ;;
    esac
    # Helper binaries a suite spawns, not suites themselves.
    case "$name" in crash_victim) continue ;; esac

    adb push "$binary" "$DEVICE_DIR/suite" >/dev/null
    adb shell chmod 755 "$DEVICE_DIR/suite"

    # TMPDIR is set because Android has no /tmp, and every one of these suites
    # builds its scratch store under it.
    out=$(adb shell "cd $DEVICE_DIR && TMPDIR=$DEVICE_DIR ./suite --test-threads=2" 2>&1 | tr -d '\r')
    result=$(printf '%s\n' "$out" | grep 'test result' | head -1)

    ran=$((ran + 1))
    case "$result" in
        *FAILED*)
            fail=1
            echo "  $name: $result"
            printf '%s\n' "$out" | grep -A4 panicked | head -12
            ;;
        *) echo "  $name: $result" ;;
    esac
done

adb shell rm -rf "$DEVICE_DIR" >/dev/null 2>&1 || true

echo
if [ "$fail" -ne 0 ]; then
    echo "android: the store does not behave the same there"
    exit 1
fi
echo "android: $ran suite(s) pass on the device"
