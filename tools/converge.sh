#!/bin/sh
# Prove a phone and a desktop reach the same archive.
#
# M3 proved convergence between two directories on one machine. That is a real
# test of the rules and says nothing about what is most likely to be wrong
# here: two platforms, two builds of SQLite, two tz databases, two notions of
# what a temporary directory is, on opposite ends of a socket.
#
# So this runs a real `capture` process against a real emulator, and asserts in
# both directions — the instrumented test checks what the phone received, and
# this checks what the desktop did, because neither can see the other's store.
#
# The phone reaches this machine at 10.0.2.2, which is what the emulator maps
# the host's loopback to. That means it dials a *remembered address* rather
# than finding the desktop over mDNS — deliberately, because the static path
# has to work on every network that blocks multicast, and would otherwise only
# ever be exercised by accident.
#
# Not `adb reverse`, which was the first attempt and cannot work: reverse makes
# adbd bind that port *on the device*, so the phone could no longer bind its
# own listener on it. The failure was honest and immediate — "Address already
# in use" from the phone's own sync — and it is an artifact of putting both
# ends on one loopback rather than anything wrong with the protocol.

set -eu

. "$(dirname "$0")/android-env.sh"
ADB="$ANDROID_HOME/platform-tools/adb"

ROOT=$(cd "$(dirname "$0")/.." && pwd)
DESK=$(mktemp -d)
CAPTURE="$ROOT/target/debug/capture"

PAIR_PORT=41823
SYNC_PORT=41822

# As the emulator sees this machine.
HOST=10.0.2.2

cleanup() {
    [ -n "${OFFER_PID:-}" ] && kill "$OFFER_PID" 2>/dev/null || true
    [ -n "${SYNCD_PID:-}" ] && kill "$SYNCD_PID" 2>/dev/null || true
    rm -rf "$DESK"
}
trap cleanup EXIT

[ -x "$CAPTURE" ] || { echo "build first: cargo build -p capture-cli"; exit 1; }
"$ADB" wait-for-device

echo "--- the desktop half, in $DESK"
export CAPTURE_HOME="$DESK"
"$CAPTURE" init >/dev/null
"$CAPTURE" + "written on the desktop" >/dev/null
DESKTOP_SLUG=$("$CAPTURE" timeline --json | sed 's/^\[{"slug":"\([^"]*\)".*/\1/')
echo "    captured $DESKTOP_SLUG"

echo "--- the daemon, so there is something to dial"
# Started before the phone syncs rather than after. The phone is the only side
# that can open a connection here — the emulator is not addressable from this
# machine — and a daemon started afterwards would be a desktop that was asleep
# for the whole exchange.
"$ROOT/target/debug/capture-syncd" >/dev/null 2>&1 &
SYNCD_PID=$!

echo "--- offering a pairing code"
OFFER_OUT=$(mktemp)
"$CAPTURE" pair > "$OFFER_OUT" 2>&1 &
OFFER_PID=$!

# Wait for the code rather than sleeping a guessed amount. A fixed sleep here
# is a test that fails on a slow machine and passes on a fast one, which is
# indistinguishable from a flaky product.
i=0
CODE=""
while [ $i -lt 50 ]; do
    CODE=$(sed -n 's/^  code   \([0-9]\{6\}\)$/\1/p' "$OFFER_OUT" | head -1)
    [ -n "$CODE" ] && break
    i=$((i + 1))
    sleep 0.2
done
[ -n "$CODE" ] || { echo "the desktop never showed a code:"; cat "$OFFER_OUT"; exit 1; }
echo "    code $CODE"

echo "--- the phone half"
cd "$ROOT/android"
./gradlew --quiet connectedDebugAndroidTest \
    -Pandroid.testInstrumentationRunnerArguments.class=dev.capture.ConvergeTest \
    -Pandroid.testInstrumentationRunnerArguments.pairingCode="$CODE" \
    -Pandroid.testInstrumentationRunnerArguments.pairingAddr="$HOST:$PAIR_PORT" \
    -Pandroid.testInstrumentationRunnerArguments.desktopSlug="$DESKTOP_SLUG"
cd "$ROOT"

wait "$OFFER_PID" || true
OFFER_PID=""

echo "--- and the other direction"
# One exchange moves events both ways, so this is not a second sync — it is
# the assertion that the desktop kept what arrived during the phone's.
kill "$SYNCD_PID" 2>/dev/null || true
SYNCD_PID=""
sleep 1

if "$CAPTURE" search "written on the phone" | grep -q .; then
    echo
    echo "converged: each device holds what the other wrote"
else
    echo
    echo "the desktop never received the phone's note"
    "$CAPTURE" timeline
    exit 1
fi
