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

What counts as a caller: a mention of the name in code — comments stripped —
that is not itself a declaration. Tests count separately, because "used only
by its own test" is precisely the state being looked for rather than a milder
version of it.

What it catches, verified by breaking each on purpose: an inherent method
whose only call site is removed, a trait method whose only call site is
removed, and a free function that nothing calls.

What it misses, stated because a check whose limits are unknown gets trusted
too far:

  - Two functions sharing a name. A caller of either satisfies both, so both
    are skipped and counted in the "not checked" line.
  - A function whose name collides with a field, variable, or type. `cosign`
    is both a command and a field of `ReadmitBody`, so the field keeps the
    command looking reachable.
  - Public types, constants, and enum variants. Only functions are examined.

Fixing the first two properly needs rustc's own name resolution rather than a
regex. That is worth doing if this ever stops earning its place; it found five
real things on its first run, and none of them needed a call graph.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ALLOW = ROOT / "tools" / "unwired-allow.txt"

# Where public API lives, and where a caller may be found.
# Files that exist to be called by tests, and nothing else.
#
# `capture-core::testing` is a fixture library behind a feature flag: every
# consumer is a test by construction, so "only tests call this" is what it is
# for rather than a finding.
SCAFFOLDING = {"crates/capture-core/src/testing.rs"}

# Crates whose callers are outside this workspace entirely.
#
# `capture-ffi` exists to be called from Kotlin. The scan cannot see that, and
# counting the generated bindings as callers would be worse than not checking:
# uniffi emits a symbol containing every method's snake_case name, so every
# method would appear called whether the application used it or not — a check
# that always passes, which is indistinguishable from no check while looking
# like one.
#
# What guards this surface instead is that it is small and enumerable by
# design, and that the application's own tests call through it. If it grows
# past what one file can hold, that is the signal it has stopped being a
# boundary.
FOREIGN_CALLERS = {"crates/capture-ffi"}

SOURCES = [
    p
    for p in sorted(ROOT.glob("crates/*/src/**/*.rs")) + sorted(ROOT.glob("apps/*/src/**/*.rs"))
    if str(p.relative_to(ROOT)) not in SCAFFOLDING
    and not any(str(p.relative_to(ROOT)).startswith(c) for c in FOREIGN_CALLERS)
]
CALLERS = SOURCES + sorted(ROOT.glob("crates/*/tests/**/*.rs")) + sorted(
    ROOT.glob("apps/*/tests/**/*.rs")
)

DEF = re.compile(r"^\s*pub(?:\(crate\))?\s+fn\s+(\w+)")

# Everything from a `//` to the end of the line. Crude about `://` inside a
# string, which costs nothing here: a name that only appears inside a URL is
# not a caller either.
CODE = re.compile(r"//.*$")

# A trait declaration, and the methods inside it. `pub` is implied on a
# trait's own methods and never written, so the ordinary pattern never saw
# them.
PUB_TRAIT = re.compile(r"^pub(?:\(crate\))?\s+trait\s+\w+")
TRAIT_FN = re.compile(r"^\s*fn\s+(\w+)")

# Any function declaration at all, public or not, including a trait
# implementation's.
ANY_FN = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)")

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
        # Depth is stored as column + 1, because a top-level `impl` or `trait`
        # sits at column zero and zero is falsy — which silently disabled both
        # of these until a trait method that plainly existed failed to appear.
        in_impl = 0
        in_trait = 0
        for line in path.read_text().splitlines():
            stripped = line.strip()
            if stripped.startswith("impl ") or stripped.startswith("impl<"):
                in_impl = line.index("impl") + 1
            elif in_impl and stripped == "}" and line.index("}") + 1 == in_impl:
                in_impl = 0

            # A trait's methods are public without saying so, which is why the
            # first version of this missed them entirely — and a trait method
            # nobody calls is exactly the shape of `Archive::owed_redactions`,
            # which was declared, implemented, and unreachable.
            if PUB_TRAIT.match(stripped):
                in_trait = line.index("pub") + 1
            elif in_trait and stripped == "}" and line.index("}") + 1 == in_trait:
                in_trait = 0

            m = DEF.match(line)
            if not m and in_trait:
                m = TRAIT_FN.match(line)
            if m:
                defined.setdefault((m.group(1), bool(in_impl or in_trait)), []).append(path)

    src = {p: p.read_text() for p in SOURCES}
    tests = {p: p.read_text() for p in CALLERS if p not in set(SOURCES)}

    def mentions(name, corpus, method):
        n = 0
        for body in corpus.values():
            for line in body.splitlines():
                if name not in line:
                    continue
                # A declaration is not a caller, and neither is an
                # implementation of one. `impl Archive for StoreArchive` names
                # every method it implements, which made each of them look
                # called by the very code that was supposed to be called *by*
                # something else.
                d = DEF.match(line) or ANY_FN.match(line)
                if d and d.group(1) == name:
                    continue
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
