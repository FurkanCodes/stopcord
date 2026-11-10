#!/usr/bin/env python3
# line.rs for a VM: plays any role on the line. Each turn runs /opt/turn.sh with its role.
# Installed in a test image as `claude`, which the VM terminal declares to the daemon.
import subprocess
import sys
import threading
import time

lock = threading.Lock()
state = {"count": 0, "text": []}


def reader():
    while True:
        line = sys.stdin.readline()
        if not line:
            return
        with lock:
            state["text"].append(line)
            state["count"] += 1


def say(s):
    sys.stdout.write(s)
    sys.stdout.flush()


def spinner(n):
    say(f"✳ Thinking… ({n}s · esc to interrupt)\r\n")


def count():
    with lock:
        return state["count"]


threading.Thread(target=reader, daemon=True).start()
role, bad = "", False
while True:
    seen = count()
    say("\x1b[2J\x1b[H> ")
    tick = time.time()
    while count() == seen:
        if time.time() - tick > 0.9:
            say("\r\n> ")
            tick = time.time()
        time.sleep(0.03)
    last = count()
    while True:
        time.sleep(0.7)
        now = count()
        if now == last:
            break
        last = now
    with lock:
        got = "".join(state["text"])
        state["text"].clear()
    if not role:
        for r in ("planner", "builder", "reviewer"):
            if f"ROLE={r}" in got:
                role = r
        bad = "GATEFAIL" in got
    spinner(1)
    child = subprocess.Popen(["/bin/sh", "/opt/turn.sh", role, "bad" if bad else "good"])
    n = 2
    while child.poll() is None:
        spinner(n)
        n += 1
        time.sleep(0.4)
    for _ in range(3):
        spinner(n)
        n += 1
        time.sleep(0.4)
