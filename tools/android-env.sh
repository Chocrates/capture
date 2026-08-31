#!/bin/sh
# Resolve and check the Android toolchain, or say exactly what is missing.
#
# Sourced by `make android`. Everything it exports is derived here rather than
# taken from the environment, so a build on one machine is the same build on
# another — the alternative is a toolchain that differs per shell and produces
# differences nobody can attribute.
#
# It fails with a list. A missing NDK otherwise surfaces as a linker error
# about a file nobody has heard of, several minutes into a build.

# The NDK this project builds against.
#
# Pinned to the release rather than resolved by picking the newest directory:
# this machine has 28.0.12433566-beta1 alongside it, and "newest" would have
# silently selected a beta toolchain.
NDK_VERSION="28.0.13004108"

# The API the binaries are compiled against. 24 rather than the app's minSdk of
# 30 because it costs nothing and keeps the native library usable if the app
# ever supports an older device than its own tile does.
NDK_API="24"

# Emulators here are x86_64; a physical phone is aarch64. Both are built, so
# `make android` cannot pass on a machine that can only run one of them.
#
# Iterated with `set --` rather than by splitting a variable. This file is
# `#!/bin/sh` but gets sourced into whatever the caller runs, and zsh does not
# word-split unquoted expansions — so the loop below saw one target named
# "x86_64-linux-android aarch64-linux-android" and reported it missing. A
# toolchain check that behaves differently per shell is the exact failure it
# exists to prevent.
set -- x86_64-linux-android aarch64-linux-android
ANDROID_TARGETS="$*"

missing=""
note() { missing="${missing}  - $1\n"; }

# --- the SDK -----------------------------------------------------------------

# Expanded with `:-` throughout. A caller running under `set -u` — which any
# careful script does — aborted here on the first unset variable, before this
# file could report anything, which is the opposite of its whole purpose.
SDK=""
for candidate in "${ANDROID_HOME:-}" "${ANDROID_SDK_ROOT:-}" "$HOME/Android/Sdk" "/usr/lib/android-sdk"; do
    if [ -n "$candidate" ] && [ -d "$candidate/platform-tools" ]; then
        SDK="$candidate"
        break
    fi
done

if [ -z "$SDK" ]; then
    note "the Android SDK. Looked in \$ANDROID_HOME, \$ANDROID_SDK_ROOT, ~/Android/Sdk and /usr/lib/android-sdk."
else
    [ -d "$SDK/platforms/android-34" ] || note "SDK platform android-34, in $SDK/platforms"
    [ -x "$SDK/platform-tools/adb" ] || note "adb, in $SDK/platform-tools"
fi

# --- the NDK -----------------------------------------------------------------

NDK="$SDK/ndk/$NDK_VERSION"
if [ ! -d "$NDK" ]; then
    note "NDK $NDK_VERSION, in $SDK/ndk. Others may be present; this is the one the build is pinned to, deliberately."
fi

# --- the toolchain -----------------------------------------------------------

command -v cargo >/dev/null 2>&1 || note "cargo"
cargo ndk --version >/dev/null 2>&1 || note "cargo-ndk. Install with: cargo install cargo-ndk"

for target in "$@"; do
    rustup target list --installed 2>/dev/null | grep -qx "$target" \
        || note "Rust target $target. Add with: rustup target add $target"
done

command -v java >/dev/null 2>&1 || note "a JDK. Android Gradle needs 17 or newer."

# --- report ------------------------------------------------------------------

if [ -n "$missing" ]; then
    echo "The Android build needs things this machine does not have:"
    printf "%b" "$missing"
    echo
    echo "Nothing above is fetched automatically: each one is a large download"
    echo "onto somebody's machine, and choosing to do that is theirs."
    exit 1
fi

export ANDROID_HOME="$SDK"
export ANDROID_SDK_ROOT="$SDK"
export ANDROID_NDK_HOME="$NDK"
export ANDROID_NDK_ROOT="$NDK"
export ANDROID_NDK_API="$NDK_API"
export ANDROID_TARGETS
