#!/bin/sh
# Install brain for the current user.
#
# Does four things, in an order chosen so that a failure part-way leaves
# something usable rather than something half-configured:
#
#   1. build          — nothing is installed until everything compiles
#   2. binaries       — onto PATH, where the shell snippet expects them
#   3. store          — created only if absent; never touched if it exists
#   4. instructions   — the two gestures, printed rather than assumed
#
# By default it does NOT edit your shell rc, arrange to start at login, or bind
# a global chord. Those are changes to your environment, and a program that
# makes them silently is one you cannot tell to stop — so it prints them.
#
# `--with-chord` opts into the two that can be done for you: starting at login,
# and binding the chord. The shell snippet still has to be pasted, because
# appending to somebody's rc file is editing a file they maintain by hand.

set -eu

WITH_CHORD=0
for arg in "$@"; do
    case "$arg" in
        --with-chord) WITH_CHORD=1 ;;
        *) printf 'unknown option: %s\n' "$arg" >&2; exit 1 ;;
    esac
done

PREFIX="${PREFIX:-$HOME/.local}"
BIN="$PREFIX/bin"
BRAIN_HOME="${BRAIN_HOME:-$HOME/brain}"

say() { printf '%s\n' "$*"; }
step() { printf '\n\033[1m%s\033[0m\n' "$*"; }

# ---------------------------------------------------------------- 1. build ---

step "Building"
if ! command -v cargo >/dev/null 2>&1; then
    say "  cargo is not installed. https://rustup.rs"
    exit 1
fi

# The desktop app needs a system webview, and the failure without it is a wall
# of linker errors rather than a sentence. Checked here so it is a sentence.
WANT_DESKTOP=1
if ! pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
    WANT_DESKTOP=0
    say "  webkit2gtk-4.1 is missing, so the browse window will be skipped."
    say "  To include it:  sudo apt install libwebkit2gtk-4.1-dev libxdo-dev \\"
    say "                       libayatana-appindicator3-dev"
    say ""
fi

cargo build --release -p brain-cli -p brain-capture -p brain-syncd
if [ "$WANT_DESKTOP" = 1 ]; then
    cargo build --release -p brain-desktop
fi

# ------------------------------------------------------------- 2. binaries ---

step "Installing to $BIN"
mkdir -p "$BIN"
for b in brain brain-capture brain-syncd; do
    install -m 755 "target/release/$b" "$BIN/$b"
    say "  $b"
done
if [ "$WANT_DESKTOP" = 1 ]; then
    install -m 755 target/release/brain-desktop "$BIN/brain-desktop"
    say "  brain-desktop"
fi

case ":$PATH:" in
    *":$BIN:"*) ;;
    *)  say ""
        say "  $BIN is not on your PATH. Add this to your shell rc:"
        say "      export PATH=\"\$PATH:$BIN\""
        ;;
esac

# ----------------------------------------------------------------- 3. store ---

step "Store"
if [ -e "$BRAIN_HOME/config/identity.key" ]; then
    # Never re-initialised. A second `init` over a real store would be a new
    # identity beside an existing log, which is the state the startup check
    # exists to detect and recover from — not one to create on purpose.
    say "  already at $BRAIN_HOME — left alone"
else
    "$BIN/brain" init
fi

# ---------------------------------------------------------- 4. the gestures ---

step "Capturing from a terminal"
say "  Add this to your shell rc, then start a new shell:"
say ""
case "${SHELL##*/}" in
    bash) say "      eval \"\$(brain shell-init bash)\"    # ~/.bashrc" ;;
    zsh)  say "      eval \"\$(brain shell-init zsh)\"     # ~/.zshrc" ;;
    *)    say "      eval \"\$(brain shell-init zsh)\"     # or bash" ;;
esac
say ""
say "  Then type at your prompt and press Ctrl-B. The line is read before the"
say "  shell touches it, so #tags, \$vars, quotes and backticks survive intact."

step "Capturing from anywhere"
if [ "$WITH_CHORD" = 1 ]; then
    "$BIN/brain-capture" --autostart

    # Started now as well as at login, so the registration below has something
    # to reach. Registering against nothing would report the chord as never
    # having worked, which is true and unhelpful on a fresh install.
    if ! "$BIN/brain-capture" --show >/dev/null 2>&1; then
        say ""
        say "  Starting it now..."
        "$BIN/brain-capture" --run >/dev/null 2>&1 &
        sleep 2
    fi

    say ""
    "$BIN/brain-capture" --register
else
    say "  Two commands, once:"
    say ""
    say "      brain-capture --autostart    be running from login"
    say "      brain-capture --register     bind the chord, then press it to confirm"
    say ""
    say "  Or re-run this with --with-chord to do both now."
    say ""
    say "  After that it behaves like the desktop's own overview: something that is"
    say "  already loaded and becomes visible, rather than something that launches."
    say "  That is what makes it instant — about 30 MB while it waits, and no"
    say "  measurable processor time."
fi

if [ "$WANT_DESKTOP" = 1 ]; then
    step "Browsing"
    say "  brain-desktop                the timeline, search and health"
fi

step "Syncing"
say "  Installed but not started. Sync is opt-in and does nothing on one device."
say ""
say "  To add a second device, on this one:"
say "      brain pair                   shows a code; type it on the other device"
say ""
say "  Then run the daemon on each:"
say "      brain-syncd                  finds paired devices and reconciles"
say ""
say "  Not started from login by default. It opens a listening socket, and that"
say "  is a thing to switch on deliberately rather than to inherit from having"
say "  run an installer."

step "Ready"
say "  brain + your first note"
say "  brain timeline"
