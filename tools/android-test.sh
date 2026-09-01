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
#
# The list of suites comes from cargo rather than from globbing the target
# directory, and that is not tidiness. Globbing ran every executable lying in
# `deps/`, which meant:
#
#   - stale binaries from earlier builds, so every suite ran twice and a
#     failure could be reported from code that no longer exists. That happened:
#     a `links` binary predating a fix failed on the device while the current
#     one passed, in the same run.
#   - binaries that are not tests at all — `uniffi_bindgen` is a command-line
#     tool, and pushing 90 MB of it to a phone to run it with `--test-threads`
#     produces no output and no meaning.
#
# A gate that tests whatever happens to be lying around is a gate that can go
# red for something already fixed, and green for something never built.

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

# Ask cargo which executables are this crate's tests. `--no-run` builds
# nothing that is already built, and the JSON says exactly which files it
# would have run.
# The suites are the crate's lib test plus one per file in tests/. That is
# what a test target *is*, so the source tree is the authority rather than
# whatever is lying in the target directory.
#
# `cargo --message-format=json` would say the same thing and is not available
# here: cargo-ndk consumes cargo's output rather than forwarding it, and plain
# cargo for this target needs the whole NDK C toolchain in the environment
# just to run a build script.
names="capture_store"
for f in crates/capture-store/tests/*.rs; do
    names="$names $(basename "$f" .rs)"
done

# The newest binary per name. Cargo leaves every previous build behind, so a
# name matches several files and only one of them is this build's.
suites=""
for name in $names; do
    newest=$(ls -t "$TARGET_DIR/$name"-* 2>/dev/null |
        grep -v '\.d$' | head -1)
    [ -n "$newest" ] || {
        echo "no binary for suite '$name'; run make android-build"
        exit 1
    }
    suites="$suites $newest"
done

fail=0
ran=0
for binary in $suites; do
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
