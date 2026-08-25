# brain — development commands.
#
# These mirror .github/workflows/ci.yml deliberately. A local check that runs a
# different set of things from the one that gates merges is worse than no local
# check: it teaches that green means safe, and then something fails anyway for a
# reason nobody can reproduce.
#
# `make` on its own lists what is here. `make ci` runs everything CI runs.

SHELL := /bin/sh
CARGO ?= cargo

# Where the demo and smoke targets put their store. Never the real one at
# ~/brain — a smoke test that writes into the archive it is testing is a smoke
# test nobody dares run twice.
SCRATCH ?= /tmp/brain-scratch-$(USER)

.DEFAULT_GOAL := help
.PHONY: help setup ci check fmt fmt-check lint test test-fast crash pty tamper roundtrip spool redact purity deny \
        build release install demo smoke tasks clean distclean

## help: list these commands
help:
	@echo 'brain — make targets'
	@echo
	@sed -n 's/^## \([a-z-]*\): \(.*\)/  \1|\2/p' $(MAKEFILE_LIST) \
	  | awk -F'|' '{printf "  \033[1m%-12s\033[0m %s\n", $$1, $$2}'
	@echo
	@echo '  Store for demo/smoke: $(SCRATCH)'

## setup: install the tools `make ci` needs
#
# Only cargo-deny, and only because it is the one gate that is a separate
# binary. `ci` deliberately refuses to run without it rather than skipping it:
# a target that quietly drops a check and still prints "passed" is the exact
# failure this Makefile exists to avoid.
setup:
	@command -v cargo-deny >/dev/null 2>&1 \
	  && echo 'cargo-deny: already installed' \
	  || $(CARGO) install cargo-deny --locked

# ---------------------------------------------------------------- gates ----

## ci: everything CI runs — do this before pushing
ci: fmt-check lint test pty tamper roundtrip spool purity deny
	@echo
	@echo 'ci: all gates passed'

## check: the fast loop — format, lint, test
check: fmt lint test

## fmt: apply formatting
fmt:
	$(CARGO) fmt --all

## fmt-check: fail if anything is unformatted (what CI does)
fmt-check:
	$(CARGO) fmt --all -- --check

## lint: clippy over everything, warnings are errors
lint:
	RUSTFLAGS='-D warnings' $(CARGO) clippy --all-targets --all-features

## test: the whole suite, including the crash harness
test:
	$(CARGO) test --workspace

## test-fast: skip the crash harness, which spawns and kills processes
test-fast:
	$(CARGO) test --workspace --lib --bins

## crash: the crash harness alone, with output
crash:
	$(CARGO) test -p brain-store --test crash -- --nocapture

## pty: drive real bash and zsh through a terminal and press the chord
#
# Separate from `make test` because it spawns interactive shells and takes
# about half a minute. It is the only thing that checks the claim the terminal
# gesture rests on, so `ci` runs it.
pty:
	$(CARGO) test -p brain-cli --test pty -- --ignored --test-threads=1

## tamper: damage a store on purpose and check `brain verify` notices
#
# Separate because it shells out to sqlite3 to corrupt real stored bytes. A
# `verify` that has never been shown a damaged archive is an assertion, not a
# check, so `ci` runs this too.
tamper:
	$(CARGO) test -p brain-cli --test tamper -- --ignored --test-threads=1

## spool: take the store away mid-capture and check nothing is lost
#
# Revokes write permission on the log, which is the same failure a full disk
# produces at the same layer, without needing a spare filesystem.
spool:
	$(CARGO) test -p brain-cli --test spool -- --ignored --test-threads=1

## redact: destroy content and check what has to survive it
redact:
	$(CARGO) test -p brain-store --test redact

## roundtrip: export a store, restore it elsewhere, compare
#
# The only check of the claim that matters on the day it matters: a restored
# store renders what the original rendered. `ci` runs it.
roundtrip:
	$(CARGO) test -p brain-cli --test roundtrip -- --ignored --test-threads=1

# ---------------------------------------------------------------- purity ----

## purity: the three-layer boundary — no_std, dependency tree, clock probe
#
# Layer three temporarily appends a clock reference to brain-core and expects
# the build to *fail*. The file is restored on every path out, including a
# failure and an interrupt: leaving a probe line in a frozen crate would be a
# genuinely bad outcome for a convenience target.
purity:
	@echo '--- brain-core builds without std'
	@$(CARGO) build -p brain-core --quiet
	@echo '--- brain-core dependency tree is clean'
	@tree=$$($(CARGO) tree -p brain-core -e normal); \
	 fail=0; \
	 for crate in tokio rusqlite getrandom jiff serde_cbor; do \
	   if printf '%s\n' "$$tree" | grep -qE "[[:space:]]$$crate v"; then \
	     echo "  $$crate must not be a dependency of brain-core"; fail=1; \
	   fi; \
	 done; \
	 [ $$fail -eq 0 ] || exit 1; \
	 echo '  clean'
	@echo '--- a clock cannot be reached from brain-core'
	@lib=crates/brain-core/src/lib.rs; \
	 backup=$$(mktemp); cp "$$lib" "$$backup"; \
	 trap 'cp "$$backup" "$$lib"; rm -f "$$backup"' EXIT HUP INT TERM; \
	 echo 'pub fn _probe() -> std::time::SystemTime { std::time::SystemTime::now() }' >> "$$lib"; \
	 if $(CARGO) build -p brain-core --quiet 2>/dev/null; then \
	   echo '  FAILED: brain-core compiled a std::time reference'; exit 1; \
	 fi; \
	 echo '  holds'

## deny: licence and dependency bans
deny:
	@command -v cargo-deny >/dev/null 2>&1 \
	  || { echo 'cargo-deny is not installed — run: make setup'; exit 1; }
	$(CARGO) deny check

# ----------------------------------------------------------------- build ----

## build: debug binaries
build:
	$(CARGO) build --workspace

## release: optimised binaries
release:
	$(CARGO) build --workspace --release

## install: put `brain` on PATH via ~/.cargo/bin
install:
	$(CARGO) install --path apps/brain-cli --force

# ------------------------------------------------------------------ demo ----

## demo: create a scratch store and capture into it
demo: build
	@rm -rf $(SCRATCH)
	@BRAIN_HOME=$(SCRATCH) ./target/debug/brain init
	@echo
	@BRAIN_HOME=$(SCRATCH) ./target/debug/brain + pick up the dry cleaning
	@BRAIN_HOME=$(SCRATCH) ./target/debug/brain + '#idea' a tool that never asks you to file anything
	@echo
	@BRAIN_HOME=$(SCRATCH) ./target/debug/brain status
	@echo
	@echo 'store: $(SCRATCH) — export BRAIN_HOME=$(SCRATCH) to poke at it'

## smoke: check the CLI contract end to end, exit codes included
#
# Exercises the promises a unit test cannot: that the binary refuses an
# unrecognised word, that it never captures one by accident, and that the four
# exit codes are actually distinct at the process boundary.
smoke: build
	@rm -rf $(SCRATCH)
	@set -e; \
	 B=./target/debug/brain; export BRAIN_HOME=$(SCRATCH); \
	 fail() { echo "  FAILED: $$1"; exit 1; }; \
	 code() { if "$$@" >/dev/null 2>&1; then echo 0; else echo $$?; fi; }; \
	 echo '--- a missing store is "not found", not a crash'; \
	 [ "$$(code $$B status)" = 2 ] || fail 'expected exit 2'; \
	 echo '--- init'; \
	 $$B init >/dev/null || fail 'init'; \
	 echo '--- capture returns a slug and exit 0'; \
	 slug=$$($$B + buy milk for the weekend) || fail 'capture'; \
	 case "$$slug" in *-*) ;; *) fail "expected a slug, got '$$slug'";; esac; \
	 echo '--- text that looks like flags is stored, not parsed'; \
	 $$B + -n 5 items --verbose >/dev/null || fail 'flag-shaped capture'; \
	 echo '--- a mistyped command is refused, never captured'; \
	 [ "$$(code $$B stauts)" = 1 ] || fail 'expected exit 1'; \
	 before=$$($$B status | awk '/^events/ {print $$2}'); \
	 $$B serach cats >/dev/null 2>&1 || true; \
	 after=$$($$B status | awk '/^events/ {print $$2}'); \
	 [ "$$before" = "$$after" ] || fail 'a refused command wrote an event'; \
	 echo '--- an empty capture is refused'; \
	 [ "$$(printf '   \n' | code $$B +)" = 1 ] || fail 'empty capture'; \
	 echo '--- a pipe is read without being asked'; \
	 echo 'from a pipe' | $$B + >/dev/null || fail 'pipe'; \
	 echo '--- output survives a reader that stops reading'; \
	 $$B timeline | head -1 >/dev/null || fail 'broken pipe'; \
	 $$B timeline 2>&1 >/dev/null | grep -q panicked && fail 'panicked on EPIPE'; \
	 echo '--- a note can be renamed and the name resolves'; \
	 $$B alias "$$slug" scratch-alias >/dev/null || fail 'alias'; \
	 $$B show scratch-alias >/dev/null || fail 'alias does not resolve'; \
	 echo '--- a name cannot be taken twice'; \
	 other=$$($$B + another note entirely here); \
	 [ "$$(code $$B alias $$other scratch-alias)" = 1 ] || fail 'alias shadowing allowed'; \
	 echo '--- a name survives an unrelated change to its note'; \
	 $$B tag scratch-alias later >/dev/null || fail 'tag'; \
	 $$B show scratch-alias >/dev/null || fail 'alias lost on re-fold'; \
	 echo '--- a rebuild keeps every note'; \
	 before=$$($$B status | awk '/^notes/ {print $$2}'); \
	 $$B rebuild >/dev/null || fail 'rebuild'; \
	 [ "$$($$B status | awk '/^notes/ {print $$2}')" = "$$before" ] || fail 'rebuild lost notes'; \
	 $$B show scratch-alias >/dev/null || fail 'rebuild lost an alias'; \
	 echo; echo 'smoke: passed'
	@rm -rf $(SCRATCH)

# ------------------------------------------------------------------ misc ----

## tasks: M1 progress by task group
tasks:
	@awk '/^## [0-9]+\./ { \
	        if (name != "") emit(); \
	        line = $$0; sub(/^## /, "", line); \
	        p = index(line, ". "); g = substr(line, 1, p - 1) + 0; \
	        name = substr(line, p + 2); d = 0; n = 0; next } \
	      /^- \[/ { n++; T++; if ($$0 ~ /^- \[x\]/) { d++; D++ } } \
	      END { emit(); printf "\n    %-46s %3d/%d\n", "TOTAL", D, T } \
	      function emit() { \
	        printf "%2d. %-46s %3d/%-4d%s\n", g, substr(name, 1, 46), d, n, (d == n ? "done" : "") }' \
	  openspec/changes/m1-single-device-capture/tasks.md

## clean: remove build artefacts
clean:
	$(CARGO) clean

## distclean: also remove the scratch store
distclean: clean
	rm -rf $(SCRATCH)
