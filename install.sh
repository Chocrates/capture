#!/bin/sh
# Install capture for the current user.
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
#
# `--with-export` installs a systemd user timer that exports daily. Also opt-in,
# for the same reason — but argued for harder than the others, because §1.2 is
# explicit that replication is not backup and the failure it covers is the one
# nothing else does.

set -eu

WITH_CHORD=0
WITH_EXPORT=0
for arg in "$@"; do
    case "$arg" in
        --with-chord) WITH_CHORD=1 ;;
        --with-export) WITH_EXPORT=1 ;;
        *) printf 'unknown option: %s\n' "$arg" >&2; exit 1 ;;
    esac
done

PREFIX="${PREFIX:-$HOME/.local}"
BIN="$PREFIX/bin"
CAPTURE_HOME="${CAPTURE_HOME:-$HOME/capture}"

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

cargo build --release -p capture-cli -p capture-overlay -p capture-syncd
if [ "$WANT_DESKTOP" = 1 ]; then
    cargo build --release -p capture-desktop
fi

# ------------------------------------------------------------- 2. binaries ---

step "Installing to $BIN"
mkdir -p "$BIN"
for b in capture capture-overlay capture-syncd; do
    install -m 755 "target/release/$b" "$BIN/$b"
    say "  $b"
done
if [ "$WANT_DESKTOP" = 1 ]; then
    install -m 755 target/release/capture-desktop "$BIN/capture-desktop"
    say "  capture-desktop"
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
if [ -e "$CAPTURE_HOME/config/identity.key" ]; then
    # Never re-initialised. A second `init` over a real store would be a new
    # identity beside an existing log, which is the state the startup check
    # exists to detect and recover from — not one to create on purpose.
    say "  already at $CAPTURE_HOME — left alone"
else
    "$BIN/capture" init
fi

# ---------------------------------------------------------- 4. the gestures ---

step "Capturing from a terminal"
say "  Add this to your shell rc, then start a new shell:"
say ""
case "${SHELL##*/}" in
    bash) say "      eval \"\$(capture shell-init bash)\"    # ~/.bashrc" ;;
    zsh)  say "      eval \"\$(capture shell-init zsh)\"     # ~/.zshrc" ;;
    *)    say "      eval \"\$(capture shell-init zsh)\"     # or bash" ;;
esac
say ""
say "  Then type at your prompt and press Ctrl-B. The line is read before the"
say "  shell touches it, so #tags, \$vars, quotes and backticks survive intact."

step "Capturing from anywhere"
if [ "$WITH_CHORD" = 1 ]; then
    "$BIN/capture-overlay" --autostart

    # Started now as well as at login, so the registration below has something
    # to reach. Registering against nothing would report the chord as never
    # having worked, which is true and unhelpful on a fresh install.
    if ! "$BIN/capture-overlay" --show >/dev/null 2>&1; then
        say ""
        say "  Starting it now..."
        "$BIN/capture-overlay" --run >/dev/null 2>&1 &
        sleep 2
    fi

    say ""
    "$BIN/capture-overlay" --register
else
    say "  Two commands, once:"
    say ""
    say "      capture-overlay --autostart    be running from login"
    say "      capture-overlay --register     bind the chord, then press it to confirm"
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
    say "  capture-desktop                the timeline, search and health"
fi

step "Syncing"
say "  Installed but not started. Sync is opt-in and does nothing on one device."
say ""
say "  To add a second device, on this one:"
say "      capture pair                   shows a code; type it on the other device"
say ""
say "  Then run the daemon on each:"
say "      capture-syncd                  finds paired devices and reconciles"
say ""
say "  Not started from login by default. It opens a listening socket, and that"
say "  is a thing to switch on deliberately rather than to inherit from having"
say "  run an installer."

step "Keeping a copy"
say "  Pairing a second device is not a backup. §1.2 is explicit about it:"
say "  replication covers a device dying and nothing else. A bad event, a"
say "  mistaken redaction, a projection gone wrong — every replica copies"
say "  those faithfully and immediately. Gossip makes corruption travel"
say "  faster, not slower. And two devices in one bag is one accident."
say ""
say "      capture export                 everything appended since the last run"
say "      capture restore <dir>          rebuild from those files"
say ""
say "  Export is incremental and takes no lock, so it is safe on a timer and"
say "  safe while you are capturing. It writes nothing when nothing is new."

if [ "$WITH_EXPORT" = 1 ]; then
    UNITS="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
    mkdir -p "$UNITS"

    # A timer rather than a cron line: it survives the machine being off.
    # `Persistent=true` runs the export on the next boot if the moment passed
    # while the machine was asleep, which is the ordinary case for a laptop and
    # exactly when somebody would otherwise silently stop having backups.
    cat > "$UNITS/capture-export.service" <<UNIT
[Unit]
Description=Export the capture archive

[Service]
Type=oneshot
Environment=CAPTURE_HOME=$CAPTURE_HOME
ExecStart=$BIN/capture export
UNIT

    cat > "$UNITS/capture-export.timer" <<UNIT
[Unit]
Description=Export the capture archive daily

[Timer]
OnCalendar=daily
Persistent=true

[Install]
WantedBy=timers.target
UNIT

    systemctl --user daemon-reload 2>/dev/null || true
    systemctl --user enable --now capture-export.timer 2>/dev/null || true

    say ""
    say "  Installed a daily timer:"
    say "      systemctl --user status capture-export.timer"
    say "      systemctl --user disable --now capture-export.timer   to stop"
    say ""
    say "  It writes into $CAPTURE_HOME/export. That directory is still on this"
    say "  one machine, which covers a mistake but not a fire — copy it"
    say "  somewhere else with whatever you already use for that."
else
    say ""
    say "  Nothing runs this for you. To have it run daily:"
    say ""
    say "      ./install.sh --with-export"
    say ""
    say "  Or add it to whatever already takes backups on this machine."
fi

step "Ready"
say "  capture + your first note"
say "  capture timeline"
