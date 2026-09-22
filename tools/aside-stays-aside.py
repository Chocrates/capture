#!/usr/bin/env python3
"""Fail when a fold or a roster is built from events the log set aside.

`Log::all` excludes events marked `aside` — authentic events this device holds
and may not use, because they were authored past the point a retirement named.
Storing them is what stops anti-entropy asking for that tail for ever; the
exclusion is what stops them being used.

`Log::all_including_aside` returns them too. It exists for the two callers that
want the archive as it physically is: an export, which must carry a set-aside
event because it occupies a position and a restore without it reproduces the
hole; and anything auditing what the device actually holds.

Handing that to a fold is a retired device's writes appearing as notes. Handing
it to `roster::build` is worse and is the reason this check exists rather than a
comment: `build` admits a `DEVICE_ADD` whose issuer's origin it knows, and a
retired origin stays known — `r.origins` keeps it so the device's past events
can still be verified. A retired device's post-head admission, folded, is a
retired device choosing a new member. The key it names is not its own, so
"retire wins" does not catch it.

The check is deliberately crude, in the manner of `unwired.py`: it reads text,
not types. What it looks for is `all_including_aside` appearing in the same
expression as a fold or a roster build, and any use of it outside the files
allowed to have one.

What it misses, stated so it is not trusted further than it goes:

  - An alias. `let everything = log.all_including_aside()?;` followed by
    `roster::build(&everything)` on another line is not caught. The allowlist is
    the real guard there: a new file using it at all has to be added here, and
    that is the moment to ask why.
  - A caller reached through a helper that takes `&[Envelope]`.

Both are cases where somebody had to write the long name on purpose. The name
is long for that reason.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Where seeing the whole archive is the point.
#
# `log.rs` defines it and tests it. `backup.rs` exports, and an export that
# skipped a set-aside event would restore to an archive with a hole in an
# origin's numbering — the one state that file's own header calls
# unrecoverable.
ALLOWED = {
    "crates/capture-store/src/log.rs",
    "apps/capture-cli/src/backup.rs",
    # Asserts the other half of the contract: that a set-aside event really is
    # stored. Without it the remedy for a retirement that was too narrow could
    # only recover the events from a peer that may be long gone.
    "crates/capture-store/tests/two_devices.rs",
}

WIDE = "all_including_aside"

# Things that must never be handed the wide view.
DERIVES = re.compile(r"(roster::build|fold::fold|projection::rebuild|incremental::apply)")


def sources():
    for base in ("crates", "apps", "tools"):
        for p in (ROOT / base).rglob("*.rs"):
            yield p


def main() -> int:
    problems = []
    for path in sources():
        rel = str(path.relative_to(ROOT))
        text = path.read_text(encoding="utf-8", errors="replace")
        if WIDE not in text:
            continue

        for n, line in enumerate(text.splitlines(), 1):
            if WIDE not in line:
                continue
            stripped = line.strip()
            if stripped.startswith("//") or stripped.startswith("///"):
                continue

            if DERIVES.search(line):
                problems.append(
                    f"{rel}:{n}: a fold or roster is built from the whole "
                    f"archive, including events this device set aside"
                )
            elif rel not in ALLOWED:
                problems.append(
                    f"{rel}:{n}: uses {WIDE}. If seeing set-aside events is "
                    f"genuinely the point here, add this file to ALLOWED in "
                    f"tools/aside-stays-aside.py and say why"
                )

    if problems:
        print("--- events set aside are being used")
        for p in problems:
            print(f"    {p}")
        print()
        print("    A set-aside event is authentic and unusable: authored past")
        print("    the point a retirement named. It is stored so that the head")
        print("    advances and anti-entropy stops re-requesting the tail, and")
        print("    it is marked so that nothing derived from the log sees it.")
        print("    `Log::all` already excludes it — use that.")
        return 1

    print(f"  {WIDE} is used only where seeing the whole archive is the point")
    return 0


if __name__ == "__main__":
    sys.exit(main())
