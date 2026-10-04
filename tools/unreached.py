#!/usr/bin/env python3
"""Fail when a window command is not reachable from the window.

`unwired.py` says in its own docstring that it cannot do this. Two functions
sharing a name defeat it, and the desktop's layering hands one operation the
same name three times on purpose —

    crates/capture-store/src/mutate.rs::redact   the store operation
    apps/capture-cli/src/write.rs::redact        the terminal command
    apps/capture-desktop/src/ipc.rs::redact      the window command

so a caller of any one of them satisfies all three. Seventy-six names are in
that state, more than half of them this exact shape, and they are exactly the
surface layer that "a tested mechanism with no caller is not a shipped feature"
was written about.

Verified by breaking it: removing `ipc::merge_entities` from the handler list
leaves `make unwired` green, because `capture-ffi` has a `merge_entities` of its
own and the scan cannot tell which definition that mention meant.

This check needs no call graph, because both sides of the window's boundary are
enumerable:

  - a command is `#[tauri::command]` over a `pub fn` in `ipc.rs`
  - it is reachable from JavaScript only if `generate_handler!` names it
  - the page calls it only by a string literal

So the three ways a command stops being a shipped feature are all decidable:

  1. defined and registered nowhere — `invoke` would fail at runtime
  2. registered and never called — a capability the window does not offer
  3. called and not registered — a typo, which is also a runtime-only failure

Case 3 is the one worth having a check for even when nothing is broken: a
mistyped command name is invisible until somebody presses the control, and the
window reports it as the store refusing rather than as a fault in the page.

The page must call by a literal for any of this to hold. A computed name would
make the scan blind, so it is a failure in itself — stated rather than silently
tolerated, which is the mistake `unwired.py` records about counting comments.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
IPC = ROOT / "apps" / "capture-desktop" / "src" / "ipc.rs"
MAIN = ROOT / "apps" / "capture-desktop" / "src" / "main.rs"
UI = sorted((ROOT / "apps" / "capture-desktop" / "ui").glob("*.js"))
ALLOW = ROOT / "tools" / "unreached-allow.txt"

# `#[tauri::command]` sits on its own line above the function it exports.
COMMAND = re.compile(r"^\s*#\[tauri::command\]")
PUB_FN = re.compile(r"^\s*pub\s+(?:async\s+)?fn\s+(\w+)")

# `ipc::name,` inside `generate_handler![ ... ]`.
REGISTERED = re.compile(r"^\s*ipc::(\w+)\s*,?\s*$")

# The page's own wrapper, which is the one place a command name may be a
# variable: its argument comes from its callers, and those are what this checks.
WRAPPER = re.compile(r"(?:async\s+)?function\s+call\s*\(")

# A call into the store, by name. Both spellings: the page funnels through
# `call` for the error handling, and the two polls use `invoke` directly.
NAMED = re.compile(r"\b(?:call|invoke)\s*\(\s*\"(\w+)\"")
ANY_CALL = re.compile(r"\b(?:call|invoke)\s*\(")


def reasons():
    """Commands exempt from being called, and the written reason for each."""
    out = {}
    if not ALLOW.exists():
        return out
    for line in ALLOW.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        name, _, why = line.partition(":")
        if not why.strip():
            print(f"{ALLOW.name}: {name.strip()} has no reason given", file=sys.stderr)
            sys.exit(2)
        out[name.strip()] = why.strip()
    return out


def commands(body: str) -> list[str]:
    """Every `#[tauri::command]` in the order it is written."""
    out, armed = [], False
    for line in body.splitlines():
        if COMMAND.match(line):
            armed = True
            continue
        if not armed:
            continue
        m = PUB_FN.match(line)
        if m:
            out.append(m.group(1))
            armed = False
    return out


def registered(body: str) -> list[str]:
    inside = body.partition("generate_handler![")[2].partition("]")[0]
    return [m.group(1) for m in (REGISTERED.match(l) for l in inside.splitlines()) if m]


def without_wrapper(body: str) -> str:
    """The page, with its `call` wrapper lifted out.

    Brace-counted, the same way `unwired.py` lifts out a `#[cfg(test)]` module.
    Removing too much would make the check blinder, so the count is checked by
    the caller: the wrapper has to be found exactly once.
    """
    m = WRAPPER.search(body)
    if not m:
        return body
    i, depth, opened = m.start(), 0, False
    while i < len(body):
        depth += body[i] == "{"
        depth -= body[i] == "}"
        if body[i] == "{":
            opened = True
        i += 1
        if opened and depth <= 0:
            break
    return body[: m.start()] + body[i:]


def main() -> int:
    exempt = reasons()
    defined = commands(IPC.read_text())
    wired = registered(MAIN.read_text())

    called: dict[str, Path] = {}
    computed: list[str] = []
    for page in UI:
        body = page.read_text()
        if len(WRAPPER.findall(body)) > 1:
            print(f"  {page.name}: more than one `call` wrapper, so the scan is blind")
            return 1
        stripped = without_wrapper(body)
        for m in NAMED.finditer(stripped):
            called.setdefault(m.group(1), page)
        # Every remaining call has to name its command outright. One that does
        # not is not a finding about a command — it is this check going blind,
        # which is worse than a finding.
        if len(ANY_CALL.findall(stripped)) != len(NAMED.findall(stripped)):
            computed.append(page.name)

    unregistered = [c for c in defined if c not in wired]
    missing = [c for c in called if c not in defined]
    unoffered = [c for c in wired if c not in called and c not in exempt]
    stale = [c for c in exempt if c not in defined]

    if not (computed or unregistered or missing or unoffered or stale):
        print(f"  {len(defined)} window commands, every one reachable from the window")
        return 0

    if computed:
        print("  the page builds a command name instead of writing it:")
        for page in computed:
            print(f"    {page}")
        print("    A computed name cannot be checked. Write the name.")
    if missing:
        print("  the page calls a command that does not exist:")
        for c in sorted(missing):
            print(f"    {called[c].name}: {c}")
        print("    This fails only when somebody presses the control.")
    if unregistered:
        print("  a command, registered nowhere:")
        for c in unregistered:
            print(f"    ipc.rs: {c} is not in main.rs's generate_handler!")
    if unoffered:
        print("  registered, and the window never calls it:")
        for c in unoffered:
            print(f"    {c}")
    if stale:
        print(f"  {ALLOW.name} names commands that no longer exist:")
        for c in sorted(stale):
            print(f"    {c}")
        print("    A reason kept past its subject excuses the next command to take the name.")

    print()
    print("  DESIGN §8.4: everything is reachable from the window.")
    print(f"  Offer it, delete it, or add it to {ALLOW.name} with a reason.")
    return 1


if __name__ == "__main__":
    sys.exit(main())
