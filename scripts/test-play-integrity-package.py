#!/usr/bin/env python3
"""Corresponding-source links must identify the exact published Git checkout."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("package_module", Path(__file__).with_name("package-module.py"))
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()


class SourceCheckoutTest(unittest.TestCase):
    def test_pins_clean_source_commit_and_rejects_local_changes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            git(root, "init", "-q")
            engine = root / "components/play-integrity"
            scripts = root / "scripts"
            engine.mkdir(parents=True)
            scripts.mkdir()
            source = engine / "main.rs"
            source.write_text("original")
            (scripts / "build-play-integrity.py").write_text("build")
            git(root, "add", ".")
            git(root, "-c", "user.name=CI", "-c", "user.email=ci@example.invalid",
                "commit", "-qm", "Initial source")
            commit = git(root, "rev-parse", "HEAD")
            package.check_source_checkout(commit, root)
            with self.assertRaisesRegex(ValueError, "commit differs"):
                package.check_source_checkout("0" * 40, root)
            source.write_text("changed after source commit")
            with self.assertRaisesRegex(ValueError, "unpublished"):
                package.check_source_checkout(commit, root)
            source.write_text("original")
            (scripts / "new-build-step.py").write_text("untracked")
            with self.assertRaisesRegex(ValueError, "unpublished"):
                package.check_source_checkout(commit, root)
            (scripts / "new-build-step.py").unlink()
            package.check_source_checkout(commit, root)

    def test_rejects_invalid_commit_identifiers(self):
        for value in (None, "", "main", "a" * 39, "z" * 40):
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, "invalid"):
                package.check_source_checkout(value)


if __name__ == "__main__":
    unittest.main()
