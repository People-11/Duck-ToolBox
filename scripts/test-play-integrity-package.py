#!/usr/bin/env python3
"""Check that a matching old manifest cannot disguise stale build inputs."""
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("package_module", Path(__file__).with_name("package-module.py"))
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


class SourceMaterialsTest(unittest.TestCase):
    def test_requires_current_sources_and_build_materials(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            engine = root / "engine"
            scripts = root / "scripts"
            engine.mkdir()
            scripts.mkdir()
            source = engine / "main.rs"
            source.write_text("original")
            for name in ["build-play-integrity.py", "test-play-integrity-native.py", "test-play-integrity-java.py", "test-play-integrity-scripts.py"]:
                (scripts / name).write_text("build material")
            path = root / "source.tar"
            with tarfile.open(path, "w") as archive:
                archive.add(source, "play-integrity/main.rs")
                for script in scripts.iterdir():
                    archive.add(script, "play-integrity/" + script.name)
            with tarfile.open(path) as archive:
                package.check_source_materials(archive, engine, scripts)
                source.write_text("edited after engine build")
                with self.assertRaisesRegex(ValueError, "out of date"):
                    package.check_source_materials(archive, engine, scripts)
                source.write_text("original")
                (engine / "new.rs").write_text("new module")
                with self.assertRaisesRegex(ValueError, "missing"):
                    package.check_source_materials(archive, engine, scripts)
                (engine / "new.rs").unlink()
                (scripts / "build-play-integrity.py").write_text("edited build input")
                with self.assertRaisesRegex(ValueError, "out of date"):
                    package.check_source_materials(archive, engine, scripts)

    def test_rejects_non_file_source_members(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "main.rs"
            source.write_text("source")
            stream = io.BytesIO()
            with tarfile.open(fileobj=stream, mode="w") as archive:
                entry = tarfile.TarInfo("play-integrity/main.rs")
                entry.type = tarfile.SYMTYPE
                entry.linkname = "/outside"
                archive.addfile(entry)
            stream.seek(0)
            with tarfile.open(fileobj=stream) as archive:
                with self.assertRaisesRegex(ValueError, "out of date"):
                    package.check_source_materials(archive, root, root)


if __name__ == "__main__":
    unittest.main()
