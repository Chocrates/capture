# Testing on a Mac you cannot copy anything off

Written for a work Mac: nothing from it may be sent anywhere, so every result
comes back as `capture doctor` codes read aloud. Those lines never hold note
text, paths, names, addresses or keys (`docs/doctor-codes.md`).

Apple silicon is assumed. Nothing here needs administrator rights, and nothing
asks for Accessibility or Input Monitoring.

## 1a. Install from CI (no toolchain needed)

Each push builds Apple-silicon binaries. On the repository's Actions page,
open the latest `ci` run and download the `capture-macos-arm64` artifact. Then:

```sh
cd ~/Downloads
unzip capture-macos-arm64.zip && tar -xzf capture-macos-arm64.tar.gz
# Unsigned, so Gatekeeper would refuse them on first run:
xattr -dr com.apple.quarantine capture-macos-arm64
mkdir -p ~/.local/bin
cp capture-macos-arm64/capture* ~/.local/bin/
~/.local/bin/capture init
```

If `~/.local/bin` is not on `PATH`, add `export PATH="$PATH:$HOME/.local/bin"`
to `~/.zshrc`. Then go on to section 2.

If management refuses to run them even so, read back `B0 blocked` (not a
doctor code — just that `capture init` would not start).

## 1b. Build and install from source

Needs Apple's command-line tools and Rust:

```sh
xcode-select -p || xcode-select --install
command -v cargo || curl https://sh.rustup.rs -sSf | sh
```

Then, in the repository:

```sh
make install
```

That builds `capture`, `capture-overlay`, `capture-syncd` and `capture-desktop`
into `~/.local/bin` and creates the store at `~/capture`. It writes **no**
LaunchAgent: nothing starts at login, nothing listens on the network.

If `~/.local/bin` is not on `PATH`, it prints the line to add to `~/.zshrc`.

If it fails to build, read back only the first line beginning `error`, minus
any path.

## 2. The round trip

```sh
capture-overlay --autostart      # LaunchAgent for the overlay; starts it now
capture-overlay --register       # press Option-Space when asked
capture doctor --ask             # answer y or n when it asks about the notification
```

Read back the lines from `capture doctor --ask`, top to bottom. Each one is
`CODE pass|fail|skip: reason`.

Expected on a working Mac with sync not installed:

```
B1 pass: store opens
B2 pass: taken and released
B3 pass: every event checks out
H1 pass: carbon hotkey
H2 pass: registered
H3 pass: seen within 14 days
O1 pass: running
O2 pass: socket answers
O3 pass: starts at login
N1 pass: notifier present
N2 pass: sent
N3 pass: seen
D1 pass: present, not launched
S1 pass: installed
S2 fail: no peer
S3 skip: nothing paired
```

S1 says `installed` because the binary is installed; it is not running. S2 and
S3 are expected to fail or skip until sync is set up, if it ever is.

## 3. What each failure most likely means here

| Line | Most likely on a work Mac | Try |
|------|---------------------------|-----|
| build fails | toolchain blocked by management | stop; nothing else can be tested |
| `B1 fail` | `CAPTURE_HOME` set to somewhere else | `unset CAPTURE_HOME` |
| `H2 fail: registration failed` | another app holds Option-Space | say which app, if known |
| `H3 fail: never seen` with H2 pass | overlay not running, or the press never arrived | check O1, press again, rerun doctor |
| `O1 fail` | overlay crashed or was never started | `capture-overlay --run` in a terminal; read back the first line it prints, minus any path |
| `O3 fail: login item written, launchd refused it` | management blocks LaunchAgents | start it by hand each login: `capture-overlay --run &` |
| `N1 fail` | `osascript` removed | nothing to do; notifications cannot work |
| `N2 fail: refused` | `osascript` blocked by management | as N1 |
| `N3 fail: not seen` | notifications for Script Editor are off | System Settings → Notifications → Script Editor → Allow |
| `D1 fail` | the desktop app did not build | read back the build's first `error` line |

The overlay is an accessory app: no Dock icon, no menu bar. Option-Space
should bring a small window to the front of whatever is focused; Escape hides
it. If it appears behind other windows, say so — that is a bug, not a setting.

## 4. Sync, only if you decide to

Sync opens a listening socket (TCP port 41822) and sends the archive to paired
devices. Whether that is acceptable on this machine is your call; the default
install never does it.

```sh
./install.sh --with-sync         # LaunchAgent for capture-syncd
```

macOS then asks whether `capture-syncd` may accept incoming connections. That
prompt is expected; an unsigned program gets it on every rebuild.

If the devices cannot find each other (mDNS is often blocked on corporate
networks), give the other device's address by hand:

```sh
capture peers add 192.168.1.20:41822
```

The next round tries it; `capture doctor` then reads S2 and S3.

To stop it again:

```sh
launchctl bootout gui/$(id -u)/dev.capture.syncd
rm ~/Library/LaunchAgents/dev.capture.syncd.plist
```

## 5. Removing everything

```sh
capture-overlay --no-autostart
rm -f ~/Library/LaunchAgents/dev.capture.*.plist
rm -f ~/.local/bin/capture ~/.local/bin/capture-overlay \
      ~/.local/bin/capture-syncd ~/.local/bin/capture-desktop
```

The store at `~/capture` is left; delete it yourself if you mean to.
