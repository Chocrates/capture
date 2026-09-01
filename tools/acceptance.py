#!/usr/bin/env python3
"""M4's acceptance run: capture, pair, sync, converge, retire — scripted.

# Why this exists rather than a list of steps in a document

Everything here was verified by hand at least once. A thing verified by hand
once is a thing nobody will verify again, and this milestone has produced a
dozen defects of the shape "built, tested, and not true of the running
program" — every one found by running something rather than reading it.

# What it does not cover, and why that is written here rather than implied

Real mDNS discovery and a real geofence firing. The emulator's network is not
the host's, so devices find each other by remembered address, and Play
Services geofencing on an emulator driven by `geo fix` delivers nothing at
all. Both need the physical device in 8.2, and a run that quietly skipped them
would read as coverage.
"""

import os
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ui

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
APP = "dev.capture"
CAPTURE = os.path.join(ROOT, "target", "debug", "capture")
SYNCD = os.path.join(ROOT, "target", "debug", "capture-syncd")
# What the emulator maps this machine's loopback to.
HOST = "10.0.2.2"

desk = tempfile.mkdtemp(prefix="capture-acceptance-")
daemon = None
offer = None


def cli(*args, home=None):
    env = dict(os.environ, CAPTURE_HOME=home or desk)
    r = subprocess.run([CAPTURE, *args], capture_output=True, text=True, env=env)
    return r.stdout.strip()


def step(name):
    print(f"--- {name}")


def check(condition, said):
    if not condition:
        print(f"    FAILED: {said}")
        cleanup()
        sys.exit(1)
    print(f"    ok: {said}")


def cleanup():
    for p in (daemon, offer):
        if p:
            p.terminate()
    subprocess.run(["rm", "-rf", desk])


def main():
    global daemon, offer

    ui.ADB = os.path.join(os.environ["ANDROID_HOME"], "platform-tools", "adb")
    ui.adb("wait-for-device")

    for binary in (CAPTURE, SYNCD):
        if not os.path.exists(binary):
            print(f"build first: cargo build -p capture-cli -p capture-syncd")
            sys.exit(1)

    step("a phone with nothing on it")
    ui.adb("shell", "pm", "clear", APP)
    ui.adb("shell", "am", "start", "-n", f"{APP}/.MainActivity")
    time.sleep(4)
    ui.wait_for("Nothing captured yet")
    check(True, "an empty archive says it is empty rather than showing a blank list")

    step("capture, from the window the tile opens")
    ui.adb("shell", "am", "start", "-n", f"{APP}/.CaptureActivity")
    time.sleep(3)
    ui.adb("shell", "input", "text", "'written%son%sthe%sphone'")
    ui.adb("shell", "input", "keyevent", "66")
    time.sleep(3)
    ui.adb("shell", "am", "start", "-n", f"{APP}/.MainActivity")
    time.sleep(3)
    ui.wait_for("written on the phone")
    check(True, "the capture is in the timeline")

    step("capture, from another application's share sheet")
    ui.adb(
        "shell",
        "am",
        "start",
        "-a",
        "android.intent.action.SEND",
        "-t",
        "text/plain",
        # Quoted for the shell on the device, which re-splits everything adb
        # concatenates. `%s` is an `input text` convention and means nothing
        # here — passing it captured the literal characters, which looked like
        # the application mangling shared text.
        "--es",
        "android.intent.extra.TEXT",
        "'shared from elsewhere'",
        "--es",
        "android.intent.extra.SUBJECT",
        "'a page'",
        "-n",
        f"{APP}/.ShareActivity",
    )
    time.sleep(4)
    ui.adb("shell", "am", "start", "-n", f"{APP}/.MainActivity")
    time.sleep(3)
    ui.wait_for("shared from elsewhere")
    check(True, "text shared from another application is captured")

    step("a desktop with something of its own")
    cli("init")
    cli("+", "written on the desktop")
    check("written-desktop" in cli("timeline", "--json"), "the desktop captured too")

    step("pairing, from the phone")
    offer = subprocess.Popen(
        [CAPTURE, "pair"],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        env=dict(os.environ, CAPTURE_HOME=desk),
    )
    code = None
    for _ in range(50):
        line = offer.stdout.readline()
        if line.strip().startswith("code"):
            code = line.split()[-1]
            break
    check(code is not None and len(code) == 6, f"the desktop is showing a code ({code})")

    ui.tap("Health")
    ui.tap("Pair a device")
    ui.type_into("Where it is", f"{HOST}:41823")
    ui.type_into("The six digits", code)
    ui.tap("Join")
    ui.wait_for("Paired with", seconds=30)
    check(True, "the phone joined the desktop's roster")

    step("sync, and convergence")
    daemon = subprocess.Popen(
        [SYNCD],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        env=dict(os.environ, CAPTURE_HOME=desk),
    )
    time.sleep(2)
    ui.tap("Health")
    ui.tap("Sync now")
    time.sleep(30)
    ui.tap("Timeline")
    ui.tap("Health")
    check(
        ui.find("Watches for", within=5) is not None,
        "the phone learned what the desktop can watch for",
    )

    ui.tap("Timeline")
    ui.wait_for("written on the desktop", seconds=20)
    check(True, "the desktop's note reached the phone")

    for _ in range(20):
        if "written on the phone" in cli("timeline"):
            break
        time.sleep(3)
    check(
        "written on the phone" in cli("timeline"),
        "and the phone's note reached the desktop",
    )
    check(
        "shared from elsewhere" in cli("timeline"),
        "including the one that came from a share sheet",
    )

    step("retirement")
    phone = None
    for line in cli("peers").splitlines():
        parts = line.split()
        if parts and len(parts[0]) == 12:
            phone = parts[0]
    check(phone is not None, f"the desktop can name the phone ({phone})")
    cli("retire", phone, "--yes")
    check(
        "retired" in cli("peers").lower() or phone not in cli("peers"),
        "the phone is no longer a member",
    )

    print()
    print("acceptance: every step passed")
    print("  not covered here, and not to be read as covered: real mDNS")
    print("  discovery and a real geofence firing. Both need a physical")
    print("  device — see 8.2.")
    cleanup()


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        cleanup()
        sys.exit(130)
