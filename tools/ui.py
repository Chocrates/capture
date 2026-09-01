#!/usr/bin/env python3
"""Drive the Android application by what is on screen.

# Why by text rather than by coordinate

Because a coordinate is a claim about a layout, and the layout moves. Driving
this by hand, a tap aimed at "Sync now" landed on "Notify me there" the moment
a peer row appeared above it — which silently set a notifier and looked, for
several minutes, like the archive had authored a setting on somebody's behalf.

An element that has moved should not be tappable at its old position, and an
element that is missing should fail loudly rather than hit whatever took its
place.
"""

import re
import subprocess
import sys
import time

ADB = None


def adb(*args, timeout=60):
    return subprocess.run(
        [ADB, *args], capture_output=True, text=True, timeout=timeout
    ).stdout


def screen():
    """Every visible text, with the centre of its box."""
    adb("shell", "uiautomator", "dump", "/sdcard/ui.xml")
    xml = adb("shell", "cat", "/sdcard/ui.xml")
    out = []
    for m in re.finditer(
        r'text="([^"]*)"[^>]*bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"', xml
    ):
        t, l, tp, r, b = m.groups()
        if t:
            out.append((t, (int(l) + int(r)) // 2, (int(tp) + int(b)) // 2))
    return out


def find(fragment, within=1):
    """The first element whose text contains this, waiting a little."""
    deadline = time.time() + within
    while True:
        for text, x, y in screen():
            if fragment in text:
                return text, x, y
        if time.time() > deadline:
            return None
        time.sleep(0.5)


def wait_for(fragment, seconds=30):
    """Wait for something to appear, or say what was there instead."""
    found = find(fragment, within=seconds)
    if found:
        return found
    print(f"    expected to see {fragment!r}; the screen said:")
    for text, _, _ in screen():
        print(f"      {text[:80]}")
    sys.exit(1)


def tap(fragment, seconds=15):
    _, x, y = wait_for(fragment, seconds)
    adb("shell", "input", "tap", str(x), str(y))
    time.sleep(1.5)


def type_into(label, value):
    """Put text in the field carrying this label.

    The label sits in the field on Material 3 until it is focused, so tapping
    the label is tapping the field. Spaces are sent as %s because `adb shell
    input text` splits on them and would silently type only the first word —
    which looked exactly like a capture being truncated by the application.
    """
    tap(label)
    adb("shell", "input", "text", f"'{value.replace(' ', '%s')}'")
    adb("shell", "input", "keyevent", "111")
    time.sleep(1)


def absent(fragment):
    return find(fragment, within=2) is None
