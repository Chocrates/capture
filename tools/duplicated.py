#!/usr/bin/env python3
"""Fail when the same function body is written more than once.

`unwired.py` refuses a public item nothing calls. This refuses the other
shape: a decision the system has already made, made again somewhere else.

# Why bodies and not names

The obvious check is "two functions with the same name", and it is wrong here.
`tag` exists three times on purpose — the operation in `capture-store`, the
verb in the command line, the command in the desktop window — and so do
`redact`, `pin`, `untag`, `search` and fifty others. That is the layering, and
a check that fires on it is a check somebody turns off.

What actually went wrong was narrower and worse: the *same body*, in four
places, with the copies beginning to disagree.

  - `local_zone` read `$TZ` and `/etc/localtime` in four files. Three trusted
    whatever `$TZ` held; the fourth validated it. A wrong answer moves quiet
    hours by the UTC offset.
  - Rendering six bytes of a key as hex appeared seven times. It is what a
    person compares when deciding whether to retire a device.
  - `unhex` appeared twice and the two disagreed about the empty string, on
    either side of the two-signature readmission of DESIGN.md §5.3.

Every one of those arrived by copying a nearby example rather than looking for
an owner. The overlay's `local_zone` exists because the command line's was
twelve lines from where somebody was already reading.

# What counts as the same

The body, with comments stripped, whitespace collapsed, and string literals
kept — two functions that differ only in a message are still two copies of the
same decision. Bodies shorter than MIN_LINES are ignored: a two-line delegation
is how a *fix* looks, and flagging it would punish the cure.
"""

import hashlib
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Below this, a shared body is a delegation or a trivial accessor rather than a
# duplicated decision — and after a dedupe, delegations are exactly what is
# left behind.
MIN_LINES = 4

# Bodies that are legitimately identical in several places.
#
# Each needs a reason. An allow-list without one becomes a place to put things
# that were too much trouble to think about.
ALLOWED = {
    # `Drop` for a test fixture: remove a directory. Several suites have their
    # own fixture deliberately (see `one-of-each`, open question 2) and the
    # teardown is one line of the same thing.
    "let _ = std::fs::remove_dir_all(&self.0);",
    #
    # Reading the wall clock in a test.
    #
    # Four suites have their own, and a shared one would be worse: a test that
    # compares against real instants needs the clock *it* reads, and routing
    # that through a helper in another file adds a place to look without
    # removing a decision. There is no decision here — it is `SystemTime::now`
    # with the error case turned into zero, and the error case cannot happen.
    #
    # This is the line the check is least sure about, which is why the reason
    # is written out rather than the entry being added quietly.
    'std::time::SystemTime::now() .duration_since(std::time::UNIX_EPOCH) '
    '.map(|d| d.as_millis() as i64) .unwrap_or(0)',
    #
    # Running the built binary and collecting both streams, in three
    # command-line suites.
    #
    # Left alone deliberately: these are end-to-end tests of the shipped
    # binary, and the value of each is that it drives the program the way a
    # person does, from its own file, with nothing shared that could make a
    # suite pass because another suite set something up.
    'let out = Command::new(capture()) .args(args) .env("CAPTURE_HOME", &self.0) '
    '.output() .expect("capture runs"); let mut text = '
    'String::from_utf8_lossy(&out.stdout).to_string(); '
    'text.push_str(&String::from_utf8_lossy(&out.stderr)); '
    '(out.status.code().unwrap_or(-1), text)',
}


def bodies(path: Path):
    """Every function body in a file, as (name, normalised, line)."""
    text = path.read_text(errors="ignore")
    lines = text.splitlines()
    out = []

    for i, line in enumerate(lines):
        m = re.match(r"\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)", line)
        if not m:
            continue
        # Walk braces from the signature to the closing one.
        depth, started, body = 0, False, []
        for cur in lines[i:]:
            depth += cur.count("{") - cur.count("}")
            if "{" in cur:
                started = True
            body.append(cur)
            if started and depth <= 0:
                break
        if not started:
            continue

        stripped = []
        for b in body[1:-1]:
            b = re.sub(r"//.*$", "", b).strip()
            if b:
                stripped.append(b)
        if len(stripped) < MIN_LINES:
            continue
        norm = " ".join(stripped)
        if norm in ALLOWED:
            continue
        out.append((m.group(1), norm, i + 1))
    return out


def main() -> int:
    tracked = subprocess.run(
        ["git", "ls-files", "*.rs"], cwd=ROOT, capture_output=True, text=True
    ).stdout.split()

    seen = defaultdict(list)
    for rel in tracked:
        p = ROOT / rel
        if not p.exists():
            continue
        for name, norm, line in bodies(p):
            seen[hashlib.blake2b(norm.encode(), digest_size=16).hexdigest()].append(
                (rel, line, name)
            )

    dupes = {k: v for k, v in seen.items() if len(v) > 1}
    if not dupes:
        print(f"  {len(seen)} function bodies, each written once")
        return 0

    print("  the same body, written more than once:")
    for places in dupes.values():
        print(f"    {places[0][2]}:")
        for rel, line, name in places:
            print(f"      {rel}:{line}  {name}")
    print()
    print("  A decision made twice is a decision that can be made differently.")
    print("  Give it one home, or add the body to ALLOWED with a reason.")
    return 1


if __name__ == "__main__":
    sys.exit(main())
