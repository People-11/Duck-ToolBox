import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("stable_version", Path(__file__).with_name("stable_version.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class StableVersionTest(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.prop = self.root / "module/module.prop"
        self.prop.parent.mkdir()
        self.prop.write_text("version=1.2.3-stable\nversionCode=10\ndescription=Original\n")
        self.git("init", "-q")
        self.git("add", ".")
        self.git("-c", "user.name=CI", "-c", "user.email=ci@example.invalid", "commit", "-qm", "Initial version")
        self.before = self.git("rev-parse", "HEAD").strip()

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root, text=True)

    def test_description_or_code_changes_do_not_publish(self):
        self.prop.write_text("version=1.2.3-stable\nversionCode=11\ndescription=MediaTek support\n")
        self.assertFalse(release.should_publish("push", self.before, self.root))

    def test_version_change_requests_release(self):
        self.prop.write_text("version=1.2.4-stable\nversionCode=11\n")
        self.assertTrue(release.should_publish("push", self.before, self.root))

    def test_manual_release_remains_explicit(self):
        self.assertTrue(release.should_publish("workflow_dispatch", None, self.root))

    def test_initial_push_and_unrelated_events_do_not_publish(self):
        for event, before in [("push", None), ("push", "0" * 40), ("pull_request", self.before)]:
            self.assertFalse(release.should_publish(event, before, self.root))

    def test_invalid_revision_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "previous revision"):
            release.should_publish("push", "--invalid", self.root)

    def test_missing_or_duplicate_version_blocks_release(self):
        for text in ["versionCode=11\n", "version=\n", "version=1\nversion=2\n"]:
            self.prop.write_text(text)
            with self.assertRaisesRegex(ValueError, "exactly one"):
                release.should_publish("push", self.before, self.root)

    def test_workflow_entrypoint_writes_the_publish_output(self):
        event = self.root / "event.json"
        output = self.root / "output"
        event.write_text(json.dumps({"before": self.before}))
        subprocess.check_call(
            [sys.executable, str(Path(release.__file__).resolve())], cwd=self.root,
            env={**os.environ, "GITHUB_EVENT_NAME": "push", "GITHUB_EVENT_PATH": str(event),
                 "GITHUB_OUTPUT": str(output)},
        )
        self.assertEqual(output.read_text(), "publish=false\n")


if __name__ == "__main__":
    unittest.main()
