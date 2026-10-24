#!/usr/bin/env python3
"""Validate and package the module, including the separately licensed engine."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import struct
import subprocess
import tarfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
MODULE = ROOT / "module"


def require(path):
    if not path.is_file() or not path.stat().st_size:
        raise ValueError(f"Missing required payload: {path.relative_to(ROOT)}")


def elf(path, width, machine):
    require(path)
    header = path.read_bytes()[:20]
    if header[:4] != b"\x7fELF" or header[4] != width or header[5] != 1 or struct.unpack_from("<H", header, 18)[0] != machine:
        raise ValueError(f"Incorrect ELF ABI: {path}")


def check_source_materials(archive, engine, scripts):
    excluded = {".gradle", ".kotlin", ".cxx", "build", "target", "module", "vendor", ".git"}
    materials = {
        "play-integrity/" + path.relative_to(engine).as_posix(): path
        for path in engine.rglob("*")
        if path.is_file() and not any(part in excluded for part in path.relative_to(engine).parts)
    }
    for name in ["build-play-integrity.py", "test-play-integrity-native.py", "test-play-integrity-java.py", "test-play-integrity-scripts.py"]:
        materials[f"play-integrity/{name}"] = scripts / name
    for name, path in materials.items():
        try:
            member = archive.getmember(name)
        except KeyError as error:
            raise ValueError(f"Corresponding source is missing {name}; rebuild the engine") from error
        if not member.isfile() or archive.extractfile(member).read() != path.read_bytes():
            raise ValueError(f"Corresponding source is out of date: {name}; rebuild the engine")


def validate():
    require(MODULE / "webroot/index.html")
    require(MODULE / "play-integrity/build.json")
    manifest = json.loads((MODULE / "play-integrity/build.json").read_text())
    upstream = json.loads((ROOT / "components/play-integrity/UPSTREAM.json").read_text())
    if manifest["upstream_revision"] != upstream["revision"]:
        raise ValueError("Engine revision differs from the recorded source")
    for name, digest in manifest["files"].items():
        path = MODULE / name
        require(path)
        if hashlib.sha256(path.read_bytes()).hexdigest() != digest:
            raise ValueError(f"Engine artifact hash mismatch: {name}")
    elf(MODULE / "bin/duckd", 2, 183)
    elf(MODULE / "play-integrity/pif-engine", 2, 183)
    elf(MODULE / "zygisk/arm64-v8a.so", 2, 183)
    elf(MODULE / "zygisk/armeabi-v7a.so", 1, 40)
    dex = MODULE / "play-integrity/classes.dex"
    if not dex.read_bytes().startswith(b"dex\n"):
        raise ValueError("Invalid DEX payload")
    for name in ["GPL-3.0.txt", "NOTICE", "UPSTREAM.json", "zygisk-ISC.txt", "HiddenApiBypass-Apache-2.0.txt", "libcxx-Apache-2.0-with-LLVM-exception.txt"]:
        require(MODULE / "licenses/play-integrity" / name)
    source = MODULE / "licenses/play-integrity-source.tar.gz"
    public = MODULE / "webroot/licenses"
    public.mkdir(exist_ok=True)
    public_source = public / source.name
    if not source.exists():
        source = public_source
    require(source)
    if hashlib.sha256(source.read_bytes()).hexdigest() != manifest["source_sha256"]:
        raise ValueError("Corresponding-source hash mismatch")
    with tarfile.open(source) as archive:
        check_source_materials(archive, ROOT / "components/play-integrity", ROOT / "scripts")
        for name in ["Cargo.toml", "Cargo.lock", "LICENSE", "NOTICE", "RULES.md", "build-play-integrity.py", "build-materials-MIT.txt", "test-play-integrity-native.py", "test-play-integrity-java.py", "test-play-integrity-scripts.py", "tests/native.cpp", "tests/jni.cpp", "tests/FieldValueTest.java", "cli/src/main.rs", "cli/src/library.rs", "cli/src/inspection.rs", "cli/tests/library.rs", "cli/tests/interface.rs", "android/app/build.gradle.kts", "scripts/stage.sh", "vendor/json/LICENSE.MIT", ".cargo/config.toml"]:
            member = archive.getmember(f"play-integrity/{name}")
            if not member.isfile() or not member.size:
                raise ValueError(f"Missing corresponding-source material: {name}")
        recorded = json.load(archive.extractfile("play-integrity/UPSTREAM.json"))
        if recorded["revision"] != manifest["upstream_revision"]:
            raise ValueError("Corresponding-source upstream revision mismatch")
        if archive.extractfile("play-integrity/build-materials-MIT.txt").read() != (ROOT / "LICENSE").read_bytes():
            raise ValueError("Build-material license notice mismatch")
    if source != public_source:
        source.replace(public_source)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default="target/aarch64-linux-android/release/duckd")
    parser.add_argument("--output")
    parser.add_argument("--channel", choices=["stable", "nightly"], help="Set archive metadata without modifying module/module.prop")
    parser.add_argument("--validate-only", action="store_true")
    args = parser.parse_args()
    binary = ROOT / args.binary
    require(binary)
    destination = MODULE / "bin/duckd"
    if binary.resolve() != destination.resolve():
        shutil.copy2(binary, destination)
    for name in ["README.md", "CHANGELOG.md", "THIRD_PARTY_NOTICES.md"]:
        shutil.copy2(ROOT / name, MODULE / name)
    for binary_path in [MODULE / "bin/duckd", MODULE / "play-integrity/pif-engine"]:
        if binary_path.exists():
            binary_path.chmod(0o755)
    validate()
    if args.validate_only:
        return
    version = next(line.split("=", 1)[1] for line in (MODULE / "module.prop").read_text().splitlines() if line.startswith("version="))
    metadata = (MODULE / "module.prop").read_text()
    name = f"duck-toolbox-{version}"
    if args.channel == "nightly":
        code = subprocess.check_output(["git", "rev-list", "--count", "HEAD"], cwd=ROOT, text=True).strip()
        name += f"-{code}-canary"
        values = {"version": f"{version} ({code}-canary)", "versionCode": code}
        metadata = "\n".join(f"{key}={values[key]}" if (key := line.partition("=")[0]) in values else line for line in metadata.splitlines()) + "\n"
    if args.channel is None:
        name += "-play-integrity"
    archive = ROOT / (args.output or f"dist/{name}.zip")
    archive.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as output:
        for path in sorted(MODULE.rglob("*")):
            if path.is_file():
                if path == MODULE / "module.prop" and args.channel:
                    output.writestr("module.prop", metadata)
                else:
                    output.write(path, path.relative_to(MODULE))
    with zipfile.ZipFile(archive) as output:
        if output.testzip() is not None:
            raise ValueError("Module archive CRC validation failed")
    print(archive)


if __name__ == "__main__":
    main()
