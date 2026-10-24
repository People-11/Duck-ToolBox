import contextlib
import io
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from telegram import MAX_DOCUMENT_BYTES, build_caption, send_notification


class CaptionTests(unittest.TestCase):
    def caption(self, **environment: str) -> str:
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory) / "module.zip"
            artifact.touch()
            with patch.dict(os.environ, environment, clear=True):
                return build_caption(str(artifact))

    def test_push_keeps_commit_metadata_and_marks_nightly(self):
        with patch("telegram.subprocess.check_output") as git:
            caption = self.caption(
                COMMIT_MESSAGE="Fix module\n\nCorrect its configuration.",
                COMMIT_AUTHOR="Contributor",
                GIT_BRANCH="main",
                GIT_SHA="12345678abcdef",
                MODULE_VERSION="0.1.2-stable (58-canary)",
            )
        git.assert_not_called()
        self.assertIn("Channel: <b>nightly</b>", caption)
        self.assertIn("Version: <b>0.1.2-stable (58-canary)</b>", caption)
        self.assertIn("Correct its configuration.", caption)
        self.assertIn("Author: <b>Contributor</b>", caption)

    def test_manual_stable_reads_metadata_from_the_built_commit(self):
        with patch(
            "telegram.subprocess.check_output",
            return_value="Release module\n\nStable build.\0Contributor\n",
        ) as git:
            caption = self.caption(
                RELEASE_CHANNEL="stable",
                MODULE_VERSION="0.1.2-stable",
                GIT_SHA="12345678abcdef",
            )
        git.assert_called_once_with(
            ["git", "show", "-s", "--format=%B%x00%an", "12345678abcdef"],
            text=True,
        )
        self.assertIn("Channel: <b>stable</b>", caption)
        self.assertIn("Version: <b>0.1.2-stable</b>", caption)
        self.assertIn("Release module", caption)
        self.assertIn("Author: <b>Contributor</b>", caption)

    def test_header_is_escaped_and_survives_caption_truncation(self):
        caption = self.caption(
            RELEASE_CHANNEL="stable",
            MODULE_VERSION="0.1.2<&>",
            COMMIT_MESSAGE="x" * 2000 + "\n\n" + "y" * 2000,
            COMMIT_AUTHOR="Contributor",
        )
        self.assertLessEqual(len(caption), 1024)
        self.assertIn("Channel: <b>stable</b>", caption)
        self.assertIn("Version: <b>0.1.2&lt;&amp;&gt;</b>", caption)


class NotificationTests(unittest.TestCase):
    asset_url = "https://github.com/owner/repo/releases/download/nightly/module.zip"

    def notification(
        self,
        size: int,
        *,
        release_asset_url: str = asset_url,
        http_code: str = "200",
        curl_exit: int = 0,
        response: str = '{"ok":true,"result":{"message_id":42}}',
        expected_exit: int | None = None,
        **environment: str,
    ):
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory) / "module.zip"
            with artifact.open("wb") as file:
                file.truncate(size)

            def request(command, **kwargs):
                response_file = Path(command[command.index("--output") + 1])
                response_file.write_text(response, encoding="utf-8")
                return subprocess.CompletedProcess(command, curl_exit, http_code, "")

            values = {
                "TELEGRAM_BOT_TOKEN": "test-token",
                "TELEGRAM_CHAT_ID": "test-chat",
                "COMMIT_MESSAGE": "Fix module\n\nCorrect its configuration.",
                "COMMIT_AUTHOR": "Contributor",
                "MODULE_VERSION": "0.1.2-stable (58-canary)",
                **environment,
            }
            output, errors = io.StringIO(), io.StringIO()
            with patch.dict(os.environ, values, clear=True), patch(
                "telegram.subprocess.run", side_effect=request
            ) as curl, contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
                if expected_exit is None:
                    send_notification(str(artifact), release_asset_url)
                else:
                    with self.assertRaises(SystemExit) as error:
                        send_notification(str(artifact), release_asset_url)
                    self.assertEqual(error.exception.code, expected_exit)
            return curl, output.getvalue(), errors.getvalue()

    def test_uploads_documents_up_to_and_including_limit(self):
        for size in (MAX_DOCUMENT_BYTES - 1, MAX_DOCUMENT_BYTES):
            with self.subTest(size=size):
                curl, output, errors = self.notification(size)
                command = curl.call_args.args[0]
                self.assertIn("https://api.telegram.org/bottest-token/sendDocument", command)
                self.assertTrue(any(value.startswith("document=@") for value in command))
                self.assertTrue(any(value.startswith("caption=Channel:") for value in command))
                self.assertIn("Telegram message_id: 42", output)
                self.assertEqual(errors, "")

    def test_oversized_document_sends_release_link_and_explanation(self):
        curl, output, errors = self.notification(MAX_DOCUMENT_BYTES + 1)
        command = curl.call_args.args[0]
        self.assertIn("https://api.telegram.org/bottest-token/sendMessage", command)
        self.assertFalse(any(value.startswith("document=@") for value in command))
        text = next(value for value in command if value.startswith("text="))
        self.assertIn(f'<a href="{self.asset_url}">module.zip</a>', text)
        self.assertIn("50 MB 上传限制", text)
        self.assertIn("50.00 MiB", text)
        self.assertIn("Version: <b>0.1.2-stable (58-canary)</b>", text)
        self.assertIn("Correct its configuration.", text)
        self.assertIn("Telegram message_id: 42", output)
        self.assertEqual(errors, "")

    def test_release_link_is_escaped_and_survives_long_commit(self):
        curl, _, _ = self.notification(
            MAX_DOCUMENT_BYTES + 1,
            release_asset_url=self.asset_url + '?a=1&b="value"',
            COMMIT_MESSAGE="x" * 2000 + "\n\n" + "y" * 2000,
        )
        text = next(value for value in curl.call_args.args[0] if value.startswith("text="))
        self.assertIn('?a=1&amp;b=&quot;value&quot;', text)
        self.assertLessEqual(len(text), 4096)

    def test_oversized_document_without_release_url_fails_before_request(self):
        curl, _, errors = self.notification(
            MAX_DOCUMENT_BYTES + 1, release_asset_url="", expected_exit=1
        )
        curl.assert_not_called()
        self.assertIn("Missing release asset URL", errors)

    def test_missing_secrets_skips_notification(self):
        curl, _, errors = self.notification(1, TELEGRAM_BOT_TOKEN="")
        curl.assert_not_called()
        self.assertIn("Missing Telegram secrets", errors)

    def test_transport_and_api_errors_fail_notification(self):
        cases = [
            ({"curl_exit": 7}, "curl exit 7"),
            ({"http_code": "413"}, "HTTP 413"),
            ({"response": '{"ok":false,"description":"Forbidden"}'}, "Forbidden"),
            ({"response": '{"ok":true,"result":{}}'}, "message_id"),
        ]
        for options, message in cases:
            with self.subTest(options=options):
                _, _, errors = self.notification(1, expected_exit=1, **options)
                self.assertIn(message, errors)
                self.assertNotIn("test-token", errors)


if __name__ == "__main__":
    unittest.main()
