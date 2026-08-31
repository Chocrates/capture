# capture — development commands.
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
# ~/capture — a smoke test that writes into the archive it is testing is a smoke
# test nobody dares run twice.
SCRATCH ?= /tmp/capture-scratch-$(USER)

.DEFAULT_GOAL := help
.PHONY: help setup ci check fmt fmt-check lint test test-fast crash pty tamper roundtrip spool redact scenarios accept purity deny \
        build release install install-chord scratch demo smoke tasks clean distclean

## help: list these commands
help:
	@echo 'capture — make targets'
	@echo
	@sed -n 's/^## \([a-z-]*\): \(.*\)/  \1|\2/p' $(MAKEFILE_LIST) \
	  | awk -F'|' '{printf "  \033[1m%-14s\033[0m %s\n", $$1, $$2}'
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
ci: fmt-check lint test pty tamper roundtrip spool redact purity unwired deny
	@echo
	@echo 'ci: all gates passed'
	@echo '     (not included: `make android` — it needs an SDK, an NDK and a'
	@echo '      JVM, and a gate that cannot run everywhere stops being run)'

## android: build the core for Android and run its suite on a device
#
# Deliberately not part of `ci`. A Gradle and NDK build needs an SDK, an NDK
# and a JVM, none of which the Rust gates need — and a `ci` that cannot run
# without an Android SDK is one that stops being run at all. The omission is
# stated here so it reads as a decision.
#
# `ci` proves the code is correct. This proves it is still correct somewhere
# that is not Linux, which is a different question and the one that matters
# once a phone holds the archive.
android: android-build android-kotlin android-decides android-test

## android-build: cross-compile the core for the emulator and a real phone
android-build:
	@echo '--- android: toolchain'
	@sh tools/android-env.sh >/dev/null
	@echo '--- android: building for x86_64 (emulator) and aarch64 (device)'
	@set -e; . ./tools/android-env.sh; \
	 cargo ndk -t x86_64 -t arm64-v8a -P $$ANDROID_NDK_API build -p capture-store; \
	 cargo ndk -t x86_64 -P $$ANDROID_NDK_API test -p capture-store --no-run

## android-kotlin: generate the Kotlin the app calls
#
# Generated from the built library rather than from a UDL file, so the Kotlin
# cannot describe a surface the Rust does not have. The generator lives in
# capture-ffi rather than being installed: a version skew between generator and
# runtime produces Kotlin that compiles and then fails on the first call, which
# is the worst place to find out.
android-kotlin:
	@echo '--- android: generating Kotlin from the built library'
	@set -e; . ./tools/android-env.sh; \
	 cargo ndk -t x86_64 -P $$ANDROID_NDK_API build -p capture-ffi; \
	 cargo run -q -p capture-ffi --bin uniffi-bindgen -- \
	   generate --library target/x86_64-linux-android/debug/libcapture_ffi.so \
	   --language kotlin --no-format --out-dir android/app/src/main/java
	@echo '    android/app/src/main/java/uniffi/capture_ffi/capture_ffi.kt'

## android-decides: fail if the Kotlin side starts deciding things
#
# The unwired lesson in another language. A second implementation of the fold,
# a slug or a date is not a duplicate — it is the one nobody exercises, and it
# diverges silently because both sides look correct in isolation.
android-decides:
	@echo '--- android: kotlin decides nothing'
	@python3 tools/kotlin-decides-nothing.py

## android-test: run the store's suite on a connected device or emulator
#
# Needs a device. Start one with:
#   $$ANDROID_HOME/emulator/emulator -avd <name> -no-window -no-audio &
android-test:
	@echo '--- android: running the store suite on the device'
	@set -e; . ./tools/android-env.sh; sh tools/android-test.sh

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
	$(CARGO) test -p capture-store --test crash -- --nocapture

## pty: drive real bash and zsh through a terminal and press the chord
#
# Separate from `make test` because it spawns interactive shells and takes
# about half a minute. It is the only thing that checks the claim the terminal
# gesture rests on, so `ci` runs it.
pty:
	$(CARGO) test -p capture-cli --test pty -- --ignored --test-threads=1

## tamper: damage a store on purpose and check `capture verify` notices
#
# Separate because it shells out to sqlite3 to corrupt real stored bytes. A
# `verify` that has never been shown a damaged archive is an assertion, not a
# check, so `ci` runs this too.
tamper:
	$(CARGO) test -p capture-cli --test tamper -- --ignored --test-threads=1

## spool: take the store away mid-capture and check nothing is lost
#
# Revokes write permission on the log, which is the same failure a full disk
# produces at the same layer, without needing a spare filesystem.
spool:
	$(CARGO) test -p capture-cli --test spool -- --ignored --test-threads=1

## accept: the M1 exit criteria — every suite that gates the milestone
#
# Everything `ci` runs, plus the scenario suite. What it cannot cover is
# written into the summary rather than left implied: two of the thirteen
# acceptance items need a person.
accept: ci scenarios
	@echo
	@echo 'M1 acceptance'
	@echo '  automated gates:  passed'
	@echo
	@echo '  still needs a person:'
	@echo '    13.5  press the chord on this compositor and see the window'
	@echo '    13.6  use it for a day; fix what makes capture feel slow'
	@echo
	@echo '  blocked:'
	@echo '    group 12  browse — needs libwebkit2gtk-4.1-dev'
	@echo '    8.7/8.8   portal and X11 backends are unreachable on this session'

## scenarios: the spec's scenarios, driven through the binary
scenarios:
	$(CARGO) test -p capture-cli --test scenarios -- --ignored --test-threads=1

## redact: destroy content and check what has to survive it
redact:
	$(CARGO) test -p capture-store --test redact

## roundtrip: export a store, restore it elsewhere, compare
#
# The only check of the claim that matters on the day it matters: a restored
# store renders what the original rendered. `ci` runs it.
roundtrip:
	$(CARGO) test -p capture-cli --test roundtrip -- --ignored --test-threads=1

## unwired: fail when something public has no caller
#
# A tested mechanism with no caller is not a shipped feature, and this project
# has lost three rounds to that exact shape — `device_retire` correct in the
# fold and authored by nothing, `honour_self` tested and never called,
# `discovery::backoff` replaced at the call site by an untested copy. Each had
# passing tests, and the passing tests are what made them look finished.
#
# Deliberately crude: it matches names, not a call graph. The crude version
# found six real things on its first run and stays something a person can read
# and argue with.
unwired:
	@echo '--- everything public has a caller'
	@python3 tools/unwired.py

# ---------------------------------------------------------------- purity ----

## purity: the three-layer boundary — no_std, dependency tree, clock probe
#
# Layer three temporarily appends a clock reference to capture-core and expects
# the build to *fail*. The file is restored on every path out, including a
# failure and an interrupt: leaving a probe line in a frozen crate would be a
# genuinely bad outcome for a convenience target.
purity:
	@echo '--- capture-core builds without std'
	@$(CARGO) build -p capture-core --quiet
	@echo '--- capture-core dependency tree is clean'
	@tree=$$($(CARGO) tree -p capture-core -e normal); \
	 fail=0; \
	 for crate in tokio rusqlite getrandom jiff serde_cbor; do \
	   if printf '%s\n' "$$tree" | grep -qE "[[:space:]]$$crate v"; then \
	     echo "  $$crate must not be a dependency of capture-core"; fail=1; \
	   fi; \
	 done; \
	 [ $$fail -eq 0 ] || exit 1; \
	 echo '  clean'
	@echo '--- a clock cannot be reached from capture-core'
	@lib=crates/capture-core/src/lib.rs; \
	 backup=$$(mktemp); cp "$$lib" "$$backup"; \
	 trap 'cp "$$backup" "$$lib"; rm -f "$$backup"' EXIT HUP INT TERM; \
	 echo 'pub fn _probe() -> std::time::SystemTime { std::time::SystemTime::now() }' >> "$$lib"; \
	 if $(CARGO) build -p capture-core --quiet 2>/dev/null; then \
	   echo '  FAILED: capture-core compiled a std::time reference'; exit 1; \
	 fi; \
	 echo '  holds'
	@echo '--- capture-time builds without std'
	@$(CARGO) build -p capture-time --quiet
	@echo '--- a clock cannot be reached from capture-time'
	@lib=crates/capture-time/src/lib.rs; \
	 backup=$$(mktemp); cp "$$lib" "$$backup"; \
	 trap 'cp "$$backup" "$$lib"; rm -f "$$backup"' EXIT HUP INT TERM; \
	 echo 'pub fn _probe() -> std::time::SystemTime { std::time::SystemTime::now() }' >> "$$lib"; \
	 if $(CARGO) build -p capture-time --quiet 2>/dev/null; then \
	   echo '  FAILED: capture-time compiled a std::time reference'; exit 1; \
	 fi; \
	 echo '  holds'
	@echo '--- chrono cannot reach a clock from capture-time'
	@if $(CARGO) tree -p capture-time -e normal -f '{p} {f}' | grep -qE 'chrono v[^ ]+ .*(^| )clock'; then \
	   echo '  FAILED: chrono is compiled with its clock feature'; exit 1; \
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

## install: build everything and install it for the current user
#
# Builds, installs to ~/.local/bin, and creates the store. It prints the two
# gestures rather than arranging them: a login entry and a desktop keybinding
# are changes to the user's environment, and a program that makes them silently
# is one they cannot tell to stop.
install:
	@./install.sh

## install-chord: the same, and set the global chord up while it is at it
#
# The opt-in half. Arranges to start at login, binds the chord, and then asks
# for it to be pressed — because "the setting was written" and "the gesture
# works" are different claims and only the second one matters.
#
# The shell snippet is still printed rather than appended. An rc file is one the
# user maintains by hand, and writing into it is a different kind of liberty
# from adding a file of our own.
install-chord:
	@./install.sh --with-chord

# --------------------------------------------------------------- scratch ----

## scratch: an empty store to test against, thrown away with rm -rf
#
# There is deliberately no command that deletes a note. Redaction destroys
# content, and `DESIGN.md` §1.1 keeps its cancellation window, its cap and its
# recovery snapshot even under `--force` — none of which you want in the middle
# of a test, and all of which exist for a reason worth not weakening for
# convenience. Every note redacted also stays in the timeline as a tombstone,
# because the log is append-only and that is the point of it.
#
# So testing gets its own store instead of a way to damage the real one. The
# whole store is one directory named by CAPTURE_HOME, and deleting notes is
# deleting it.
scratch: build
	@rm -rf $(SCRATCH)
	@CAPTURE_HOME=$(SCRATCH) ./target/debug/capture init >/dev/null
	@echo 'empty store at $(SCRATCH)'
	@echo
	@echo 'Use it:'
	@echo '  export CAPTURE_HOME=$(SCRATCH)'
	@echo
	@echo 'The overlay takes it too, and binds its own socket under it — so'
	@echo 'this one runs alongside the real one rather than replacing it:'
	@echo '  CAPTURE_HOME=$(SCRATCH) ./target/debug/capture-overlay --run &'
	@echo '  CAPTURE_HOME=$(SCRATCH) ./target/debug/capture-overlay --show'
	@echo
	@echo 'Throw it away:'
	@echo '  make scratch      # or: rm -rf $(SCRATCH)'

# ------------------------------------------------------------------ demo ----

## demo: create a scratch store and capture into it
demo: build
	@rm -rf $(SCRATCH)
	@CAPTURE_HOME=$(SCRATCH) ./target/debug/capture init
	@echo
	@CAPTURE_HOME=$(SCRATCH) ./target/debug/capture + pick up the dry cleaning
	@CAPTURE_HOME=$(SCRATCH) ./target/debug/capture + '#idea' a tool that never asks you to file anything
	@echo
	@CAPTURE_HOME=$(SCRATCH) ./target/debug/capture status
	@echo
	@echo 'store: $(SCRATCH) — export CAPTURE_HOME=$(SCRATCH) to poke at it'

## smoke: check the CLI contract end to end, exit codes included
#
# Exercises the promises a unit test cannot: that the binary refuses an
# unrecognised word, that it never captures one by accident, and that the four
# exit codes are actually distinct at the process boundary.
smoke: build
	@rm -rf $(SCRATCH)
	@set -e; \
	 B=./target/debug/capture; export CAPTURE_HOME=$(SCRATCH); \
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
