#!/usr/bin/env python3
"""Fail when something public has no caller.

A tested mechanism with no caller is not a shipped feature. This project has
now lost three rounds to that exact shape: `device_retire` was correct in the
fold and authored by nothing, `Roster::honour_self` was tested and never
called, `discovery::backoff` was replaced at the call site by an untested
copy. Every one had passing tests, and passing tests are what made them look
finished.

The check is deliberately crude — names, not a call graph. A real call graph
would need rustc's own resolution, and the crude version already finds the
things that matter while staying something anybody can read and correct.

What counts as a caller: a mention of the name anywhere in the workspace that
is not the definition itself. Tests count as callers only for items the
allowlist says are test-only on purpose; otherwise "used only by its own test"
is precisely the state being looked for.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ALLOW = ROOT / "tools" / "unwired-allow.txt"

# Where public API lives, and where a caller may be found.
# Files that exist to be called by tests, and nothing else.
#
# `brain-core::testing` is a fixture library behind a feature flag: every
# consumer is a test by construction, so "only tests call this" is what it is
# for rather than a finding.
SCAFFOLDING = {"crates/brain-core/src/testing.rs"}

SOURCES = [
    p
    for p in sorted(ROOT.glob("crates/*/src/**/*.rs")) + sorted(ROOT.glob("apps/*/src/**/*.rs"))
    if str(p.relative_to(ROOT)) not in SCAFFOLDING
]
CALLERS = SOURCES + sorted(ROOT.glob("crates/*/tests/**/*.rs")) + sorted(
    ROOT.glob("apps/*/tests/**/*.rs")
)

DEF = re.compile(r"^\s*pub(?:\(crate\))?\s+fn\s+(\w+)")

# Everything from a `//` to the end of the line. Crude about `://` inside a
# string, which costs nothing here: a name that only appears inside a URL is
# not a caller either.
CODE = re.compile(r"//.*$")

# Names a crude scan cannot resolve, and why each is exempt. Anything here is
# a claim somebody made in writing, which is the point of the file.
def allowed():
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


def main() -> int:
    exempt = allowed()
    show_blind = "--blind" in sys.argv

    # Every public function, the file it is defined in, and whether it is a
    # method. The distinction is what makes the scan usable: a method is called
    # with a dot and a free function is not, so `Session::retire` and the
    # command `pair::retire` stop masking each other — which they did, and
    # which made the check pass while retirement was unreachable.
    defined: dict[tuple[str, bool], list[Path]] = {}
    for path in SOURCES:
        in_impl = 0
        for line in path.read_text().splitlines():
            stripped = line.strip()
            if stripped.startswith("impl ") or stripped.startswith("impl<"):
                in_impl = line.index("impl")
            elif in_impl and stripped == "}" and line.index("}") == in_impl:
                in_impl = 0
            m = DEF.match(line)
            if m:
                defined.setdefault((m.group(1), bool(in_impl)), []).append(path)

    src = {p: p.read_text() for p in SOURCES}
    tests = {p: p.read_text() for p in CALLERS if p not in set(SOURCES)}

    def mentions(name, corpus, method):
        n = 0
        for body in corpus.values():
            for line in body.splitlines():
                if name not in line:
                    continue
                m = DEF.match(line)
                if m and m.group(1) == name:
                    continue  # the definition itself
                # Comments do not call anything. Counting them was the first
                # version's mistake and it made the check useless on exactly
                # the names it was written for: `retire` appears in a dozen
                # explanations of what retirement is, so removing its only
                # real caller changed nothing the scan could see.
                code = CODE.sub("", line)
                if re.search(rf"\b{re.escape(name)}\b", code):
                    n += 1
        return n

    # Counted separately, because "called only by its own test" is not a
    # weaker version of the problem — it is the problem. Every defect this
    # check exists for had tests, and that is what made it look finished.
    dead, tested_only, ambiguous = [], [], []
    for (name, method), where in sorted(defined.items()):
        if name in exempt:
            continue
        at = where[0].relative_to(ROOT)

        # Two functions with one name cannot be told apart by a scan that
        # matches names. Reported rather than silently treated as reachable:
        # `Session::retire` and the command that calls it are both `retire`,
        # so the command's own call site made the store method look wired even
        # when nothing called it. A check with a blind spot should say where.
        if len(where) > 1:
            ambiguous.append((name, [w.relative_to(ROOT) for w in where]))
            if show_blind:
                print(f"    blind: {name} in {', '.join(str(w.relative_to(ROOT)) for w in where)}")
            continue

        if mentions(name, src, method) > 0:
            continue
        (tested_only if mentions(name, tests, method) > 0 else dead).append((name, at))

    if ambiguous:
        # Stated, not listed. The limitation is real and worth knowing about;
        # seventy lines of it on every run is how a check gets ignored.
        print(
            f"  {len(ambiguous)} name(s) defined in more than one place and not checked "
            "(a caller of either satisfies both) \u2014 tools/unwired.py --blind to list"
        )

    if not dead and not tested_only:
        print(f"  {len(defined)} public functions, all reachable")
        return 0

    if dead:
        print("  public, and nothing mentions them at all:")
        for name, path in dead:
            print(f"    {path}: {name}")
    if tested_only:
        print("  public, and called only by tests:")
        for name, path in tested_only:
            print(f"    {path}: {name}")
    print()
    print("  A tested mechanism with no caller is not a shipped feature.")
    print("  Wire it, delete it, or add it to tools/unwired-allow.txt with a reason.")
    return 1


if __name__ == "__main__":
    sys.exit(main())
