#!/usr/bin/env python3
"""Publish a stable release only for an explicit dispatch or version change."""
import json
import os
from pathlib import Path
import re
import subprocess


def version(text):
    values = [line.partition("=")[2].strip() for line in text.splitlines()
              if line.startswith("version=")]
    if len(values) != 1 or not values[0]:
        raise ValueError("module.prop must contain exactly one nonempty version")
    return values[0]


def should_publish(event, before, root):
    if event == "workflow_dispatch":
        return True
    if event != "push" or not before or before == "0" * 40:
        return False
    if not re.fullmatch(r"[0-9a-f]{40}", before):
        raise ValueError("Invalid previous revision")
    previous = subprocess.check_output(
        ["git", "show", f"{before}:module/module.prop"], cwd=root, text=True,
    )
    current = (root / "module/module.prop").read_text()
    return version(previous) != version(current)


def main():
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
    publish = should_publish(os.environ["GITHUB_EVENT_NAME"], event.get("before"), Path.cwd())
    with Path(os.environ["GITHUB_OUTPUT"]).open("a") as output:
        output.write(f"publish={str(publish).lower()}\n")


if __name__ == "__main__":
    main()
