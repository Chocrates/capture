# brain

A place to put things you will otherwise lose, built so that the putting costs
nothing.

Most note systems ask you to decide where something goes before you can write it
down. That decision is the tax, and it is charged at the exact moment you have
the least attention to pay it — mid-sentence, mid-meeting, halfway out the door.
This one takes the text and sorts it out later, or never.

## Install

```sh
make install          # build, install to ~/.local/bin, create the store
make install-chord    # the same, and set the global chord up too
```

`install` prints the two commands that arrange the chord rather than running
them — a login entry and a desktop keybinding are changes to your environment,
and a program that makes them silently is one you cannot tell to stop.
`install-chord` opts into both, and ends by asking you to press the chord so
that "it was configured" and "it works" are not confused.

Either way the shell snippet is printed, never appended. Your rc file is one you
maintain by hand.

The browse window needs a system webview. Without it the rest still installs:

```sh
sudo apt install libwebkit2gtk-4.1-dev libxdo-dev libayatana-appindicator3-dev
```

## Capture

Three ways in. They differ in where you are, not in what they do.

**From a terminal** — add this to your shell rc, then start a new shell:

```sh
eval "$(brain shell-init zsh)"     # or bash, in ~/.bashrc
```

Type at your prompt and press **Ctrl-B**. The line clears; that is the receipt.
If it stays, the capture failed and your text is still there.

This is a keybinding rather than a command, and the distinction is the whole
point. By the time any command runs, the shell has stripped everything after a
`#` as a comment, redirected at `>`, expanded `$VAR`, and *executed* backticks.
A binding reads the line before the shell parses a byte of it, so:

```
fix auth #client-acme      →  stored whole, tag and all
revenue > costs Q3         →  no file created
run `date` before deploy   →  nothing executed
```

**From anywhere** — bind a global chord. Two commands, once:

```sh
brain-capture --autostart    # be running from login
brain-capture --register     # bind Ctrl-Super-Space, then press it to confirm
```

After that it works the way the desktop's own overview does. Pressing Super does
not launch `gnome-shell`; the shell is already running and reveals a surface it
built at login. This is the same arrangement, and it is the only one that is
instant — a window created on a keypress pays for process start, a GPU context
and font loading first, which is a visible pause on a gesture that has to feel
like it already happened.

While it waits it costs about 30 MB of private memory and no measurable
processor time. `brain-capture --no-autostart` undoes it; the terminal chord is
unaffected either way.

Registering ends by asking you to press the chord, because "the setting was
written" and "the gesture works" are different claims and only the second one
matters.

**From a script**

```sh
brain + some text                  # everything after + is content
echo "from a pipe" | brain +
brain + -- --looks-like-a-flag     # stored as text
```

## Read

```sh
brain timeline              # newest first, grouped by the day you captured
brain show <name>           # one note, with its links
brain search <words>        # full text, most relevant first
brain links <name>          # what it points at, and what points at it
brain-desktop               # the same, in a window
```

Every read command takes `--json`.

## Change

```sh
brain edit <name> <text>    # the old version is kept
brain tag <name> <tag>
brain alias <name> <new>    # a chosen name
brain pin <name>
brain mute <name>           # stays in the timeline, leaves search
brain mute '#tag'           # everything carrying it, now or later
brain resolve <name>        # dismiss a divergence; both versions stay
```

Nothing here removes anything. Every command appends an event, and the current
state is computed from the log.

## Destroy

One command does, and it is the only irreversible thing here.

```sh
brain redact <name>                 # names what will go; type the name to confirm
brain redact --cancel <id>          # within the window
brain redact --cancel --all-since <t>
```

Thirty minutes pass before anything is destroyed, and the content stays readable
throughout — otherwise cancelling would have nothing to save. After that the
payload is gone from the log, the projection and the search index.

Two things it cannot reach, and says so:

- **Export files already written.** They get copied offsite by tools this
  program has no access to. `brain status` reports the gap until you say you
  have dealt with it.
- **Anything you copied elsewhere by hand.**

## Look after it

```sh
brain status     # what the store holds, and anything needing attention
brain verify     # every id, signature, body hash and origin chain
brain export     # incremental, into ~/brain/export
brain restore <dir>
```

`status` reads counters and opens no event, so it names `verify` for the rest
rather than implying the archive is intact. `verify` exits **3** on damage —
its own code, because a script has to tell "you typed it wrong" from "the
archive is broken".

Export is manual, never scheduled. Scheduling it would invite the belief that
backups are handled; copying `~/brain/export` somewhere else is your job and an
ordinary backup tool's.

## What is where

```
~/brain/
  events.db      the log — the only thing that cannot be recomputed
  state.N.db     the projection, rebuilt from the log at will
  ops.db         local state: this device's bookkeeping
  spool          captures taken while the store was unwritable
  config/        the device key, mode 0600
  export/        what you copy offsite
```

If `state.N.db` is lost, `brain rebuild` recreates it. If `events.db` is lost,
restore from an export — nothing else can bring it back.

## Building on it

```sh
make            # the target list
make check      # format, lint, test
make ci         # every gate, as CI runs them
make accept     # the M1 exit criteria, and what a machine cannot check
```

`DESIGN.md` is the design of record and explains *why* rather than *what*. Where
the code and that document disagreed during construction, the document was
amended and the reason recorded — three times so far.

## What this is not

No sync yet: one device. No calendar, tasks or recurrence yet. No scoring, no
streaks, no completion percentages — counts describe the data, never the person.
No cloud, no account, no telemetry. The clipboard is never read.
