#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path


MAX_DOCUMENT_BYTES = 50 * 1024 * 1024


def _escape_html(text: str) -> str:
    return (
        text.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
    )


def _truncate(text: str, limit: int) -> str:
    if limit <= 0:
        return ""
    if len(text) <= limit:
        return text
    if limit <= 3:
        return text[:limit]
    return text[: limit - 3].rstrip() + "..."


def _first_paragraph(lines: list[str]) -> str:
    paragraphs: list[str] = []
    current: list[str] = []
    for raw in lines:
        line = raw.strip()
        if not line:
            if current:
                paragraphs.append(" ".join(current))
                current = []
            continue
        current.append(line)
    if current:
        paragraphs.append(" ".join(current))
    return paragraphs[0] if paragraphs else ""


def _render_caption(
    subject_text: str,
    summary_text: str,
    commit_author: str,
    git_branch: str,
    short_sha: str,
    action_url: str,
    release_header: str,
) -> str:
    commit_lines = [subject_text] if subject_text else []
    if summary_text:
        commit_lines.extend(["", summary_text])
    commit_block = _escape_html("\n".join(commit_lines))

    lines = [
        release_header,
        "",
        "Commit:",
        f"<pre>{commit_block}</pre>",
        f"Author: <b>{_escape_html(commit_author)}</b>",
        f"Branch: <b>{_escape_html(git_branch)}@{_escape_html(short_sha)}</b>",
        f'GitHub Action: <a href="{_escape_html(action_url)}">Open run</a>',
    ]
    full_sha = os.environ.get("GIT_SHA", "").strip()
    if len(full_sha) == 40 and all(c in "0123456789abcdef" for c in full_sha):
        source_url = f"https://github.com/People-11/Duck-ToolBox/tree/{full_sha}/components/play-integrity"
        lines.append(f'<a href="{source_url}">Corresponding GPLv3 source</a>')
    return "\n".join(lines)


def build_caption(artifact_path: str) -> str:
    commit_message = os.environ.get("COMMIT_MESSAGE", "").strip()
    commit_author = os.environ.get("COMMIT_AUTHOR", "").strip()
    git_branch = os.environ.get("GIT_BRANCH", "").strip() or "unknown"
    git_sha = os.environ.get("GIT_SHA", "").strip()
    action_url = os.environ.get("GITHUB_ACTION_URL", "").strip()
    channel = os.environ.get("RELEASE_CHANNEL", "nightly").strip()
    version = os.environ.get("MODULE_VERSION", "unknown").strip()
    release_header = (
        f"Channel: <b>{_escape_html(channel)}</b>\n"
        f"Version: <b>{_escape_html(version)}</b>"
    )

    if not artifact_path or not Path(artifact_path).is_file():
        print("Module artifact was not downloaded successfully.", file=sys.stderr)
        sys.exit(1)

    if not commit_message or not commit_author:
        details = subprocess.check_output(
            ["git", "show", "-s", "--format=%B%x00%an", git_sha or "HEAD"],
            text=True,
        ).split("\0", 1)
        commit_message = commit_message or details[0].strip()
        commit_author = commit_author or details[1].strip()

    if commit_message:
        lines = commit_message.splitlines()
        subject = lines[0].strip()
        body_lines = lines[1:]
    else:
        subject = f"Build artifact: {Path(artifact_path).name}"
        body_lines = []

    summary = _first_paragraph(body_lines)
    short_sha = git_sha[:7] if git_sha else "unknown"

    max_caption_length = 1024
    caption = _render_caption(
        subject, summary, commit_author, git_branch, short_sha, action_url, release_header
    )

    if len(caption) > max_caption_length and summary:
        without_summary = _render_caption(
            subject, "", commit_author, git_branch, short_sha, action_url, release_header
        )
        summary_budget = max_caption_length - len(without_summary) - 2
        summary = _truncate(summary, summary_budget)
        caption = _render_caption(
            subject, summary, commit_author, git_branch, short_sha, action_url, release_header
        )

    if len(caption) > max_caption_length:
        without_subject = _render_caption(
            "", summary, commit_author, git_branch, short_sha, action_url, release_header
        )
        subject_budget = max_caption_length - len(without_subject)
        subject = _truncate(subject, subject_budget)
        caption = _render_caption(
            subject, summary, commit_author, git_branch, short_sha, action_url, release_header
        )

    return caption


def send_notification(artifact_path: str, release_asset_url: str) -> None:
    token = os.environ.get("TELEGRAM_BOT_TOKEN", "")
    chat_id = os.environ.get("TELEGRAM_CHAT_ID", "")
    if not token or not chat_id:
        print("Missing Telegram secrets; skipping notification.", file=sys.stderr)
        return

    caption = build_caption(artifact_path)
    artifact = Path(artifact_path)
    size = artifact.stat().st_size
    fields = {"chat_id": chat_id, "parse_mode": "HTML"}
    if size > MAX_DOCUMENT_BYTES:
        if not release_asset_url:
            print("Missing release asset URL for oversized module.", file=sys.stderr)
            sys.exit(1)
        method = "sendMessage"
        fields["text"] = (
            f"{caption}\n\n"
            f"Module ZIP size: {size / (1024 * 1024):.2f} MiB. "
            "This exceeds Telegram Bot's 50 MB upload limit. "
            "Download the module from GitHub Releases instead.\n"
            f'<a href="{_escape_html(release_asset_url)}">'
            f"{_escape_html(artifact.name)}</a>"
        )
    else:
        method = "sendDocument"
        fields["caption"] = caption

    with tempfile.TemporaryDirectory() as directory:
        response_file = str(Path(directory) / "response.json")
        command = [
            "curl",
            "--silent",
            "--show-error",
            "--output", response_file,
            "--write-out", "%{http_code}",
            "-X", "POST",
            f"https://api.telegram.org/bot{token}/{method}",
        ]
        for name, value in fields.items():
            command.extend(["--form-string", f"{name}={value}"])
        if method == "sendDocument":
            command.extend(["--form", f"document=@{artifact_path}"])

        response = subprocess.run(command, capture_output=True, text=True)
        if response.returncode:
            print(
                f"Telegram request failed (curl exit {response.returncode}).",
                file=sys.stderr,
            )
            sys.exit(1)
        if response.stdout != "200":
            print(f"Telegram API returned HTTP {response.stdout}.", file=sys.stderr)
            print(Path(response_file).read_text(encoding="utf-8"), file=sys.stderr)
            sys.exit(1)
        validate_response(response_file)


def validate_response(response_file: str) -> None:
    try:
        with open(response_file, "r", encoding="utf-8") as fh:
            payload = json.load(fh)
    except (OSError, json.JSONDecodeError) as exc:
        print(f"Failed to read response file: {exc}", file=sys.stderr)
        sys.exit(1)

    if not payload.get("ok"):
        print(
            payload.get("description", "Telegram API returned ok=false"),
            file=sys.stderr,
        )
        sys.exit(1)

    result = payload.get("result") or {}
    message_id = result.get("message_id")
    if message_id is None:
        print(
            "Telegram API succeeded but did not return a message_id.", file=sys.stderr
        )
        sys.exit(1)

    print(f"Telegram message_id: {message_id}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    caption_parser = subparsers.add_parser(
        "build-caption", help="Build Telegram caption"
    )
    caption_parser.add_argument(
        "--artifact-path", required=True, help="Path to the module zip artifact"
    )

    send_parser = subparsers.add_parser(
        "send", help="Upload the module or send its release download link"
    )
    send_parser.add_argument("--artifact-path", required=True)
    send_parser.add_argument("--release-asset-url", default="")

    validate_parser = subparsers.add_parser(
        "validate-response", help="Validate Telegram API response"
    )
    validate_parser.add_argument(
        "--response-file", required=True, help="Path to the JSON response file"
    )

    args = parser.parse_args()

    if args.command == "build-caption":
        print(build_caption(args.artifact_path), end="")
    elif args.command == "send":
        send_notification(args.artifact_path, args.release_asset_url)
    elif args.command == "validate-response":
        validate_response(args.response_file)


if __name__ == "__main__":
    main()
