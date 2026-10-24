#!/usr/bin/env python3
"""Build the independent GPL engine, its notices and corresponding source archive."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import urllib.request

SCRIPT_DIR = Path(__file__).resolve().parent
STANDALONE = (SCRIPT_DIR / "android").is_dir()
ROOT = SCRIPT_DIR if STANDALONE else SCRIPT_DIR.parent
ENGINE = ROOT if STANDALONE else ROOT / "components/play-integrity"
VENDOR = ENGINE / "vendor" if STANDALONE else ROOT / "target/play-integrity/vendor"
DEPENDENCIES = {
    "json": ("nlohmann/json", "55f93686c01528224f448c19128836e7df245f72"),
    "local_cxa_atexit_finalize_impl": (
        "5ec1cff/local_cxa_atexit_finalize_impl", "b269136606d6f230089fbf47a0b219f7f2ccef3c"
    ),
    "shadowhook": ("bytedance/android-inline-hook", "854c775c2c3676e57a0f383597ebf420b5204161"),
    "hiddenapibypass": ("LSPosed/AndroidHiddenApiBypass", "4efc64fcada7df15e7b5ba068e774343b760270d"),
}


def run(*args, **kwargs):
    subprocess.run([str(arg) for arg in args], check=True, **kwargs)


def dependencies():
    VENDOR.mkdir(parents=True, exist_ok=True)
    for name, (repo, commit) in DEPENDENCIES.items():
        destination = VENDOR / name
        if (destination / ".revision").exists() and (destination / ".revision").read_text() == commit:
            continue
        archive = VENDOR / f"{name}-{commit}.tar.gz"
        if not archive.exists():
            with urllib.request.urlopen(f"https://codeload.github.com/{repo}/tar.gz/{commit}", timeout=60) as response:
                archive.write_bytes(response.read())
        if destination.exists():
            shutil.rmtree(destination)
        destination.mkdir()
        with tarfile.open(archive) as source:
            for member in source.getmembers():
                parts = Path(member.name).parts[1:]
                if not parts:
                    continue
                member.name = "/".join(parts)
                source.extract(member, destination, filter="data")
        (destination / ".revision").write_text(commit)
    # PIF deliberately disables shadowhook's linker initialization. Keep the patch
    # in the vendored source archive, rather than downloading unrecorded build inputs.
    linker = VENDOR / "shadowhook/shadowhook/src/main/cpp/sh_linker.c"
    value = linker.read_text()
    if "sh_linker_init_dummy" not in value:
        linker.write_text(value.replace("sh_linker_init", "sh_linker_init_dummy"))


def source_archive(destination):
    rust_vendor = ROOT / "target/play-integrity/rust-vendor"
    run("cargo", "vendor", "--locked", "--manifest-path", ENGINE / "Cargo.toml", rust_vendor,
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    with tarfile.open(destination, "w:gz") as archive:
        for path in ENGINE.rglob("*"):
            relative = path.relative_to(ENGINE)
            if path.is_dir() or any(part in {".gradle", ".kotlin", ".cxx", "build", "target", "module", "vendor", ".git"} for part in relative.parts):
                continue
            archive.add(path, "play-integrity/" + relative.as_posix(), recursive=False)
        archive.add(Path(__file__), "play-integrity/build-play-integrity.py")
        for name in ["test-play-integrity-native.py", "test-play-integrity-java.py", "test-play-integrity-scripts.py"]:
            archive.add(SCRIPT_DIR / name, f"play-integrity/{name}")
        materials_license = SCRIPT_DIR / "build-materials-MIT.txt" if STANDALONE else ROOT / "LICENSE"
        archive.add(materials_license, "play-integrity/build-materials-MIT.txt")
        for name in DEPENDENCIES:
            archive.add(VENDOR / name, f"play-integrity/vendor/{name}")
        archive.add(rust_vendor, "play-integrity/vendor/rust")
        config = b'[source.crates-io]\nreplace-with = "vendored-sources"\n[source.vendored-sources]\ndirectory = "vendor/rust"\n'
        entry = tarfile.TarInfo("play-integrity/.cargo/config.toml")
        entry.size = len(config)
        archive.addfile(entry, io.BytesIO(config))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--host-only", action="store_true")
    parser.add_argument("--dependencies-only", action="store_true")
    parser.add_argument("--sdk", default=os.environ.get("ANDROID_HOME", str(Path.home() / "Library/Android/sdk")))
    args = parser.parse_args()
    dependencies()
    if args.dependencies_only:
        return
    env = {**os.environ, "PIF_VENDOR_DIR": str(VENDOR), "ANDROID_HOME": args.sdk}
    run("cargo", "build", "--locked", "--manifest-path", ENGINE / "Cargo.toml", "--target-dir", ROOT / "target/pif-engine", env=env)
    if args.host_only:
        return
    ndk = Path(args.sdk) / "ndk/30.0.16248370"
    if not ndk.is_dir():
        raise SystemExit(f"Install the pinned NDK first: sdkmanager 'ndk;{ndk.name}'")
    toolchains = list((ndk / "toolchains/llvm/prebuilt").glob("*/bin"))
    if not toolchains:
        raise SystemExit("NDK host toolchain missing")
    toolchain = toolchains[0]
    target = "aarch64-linux-android"
    linker = toolchain / (f"{target}26-clang.cmd" if os.name == "nt" else f"{target}26-clang")
    env.update({
        "CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER": str(linker),
        "CC_aarch64_linux_android": str(linker),
        "AR_aarch64_linux_android": str(toolchain / ("llvm-ar.exe" if os.name == "nt" else "llvm-ar")),
    })
    run("cargo", "build", "--locked", "--release", "--manifest-path", ENGINE / "Cargo.toml", "--target-dir", ROOT / "target/pif-engine", "--target", target, env=env)
    wrapper = ENGINE / "android/gradlew"
    gradle_command = [wrapper.with_suffix(".bat")] if os.name == "nt" else ["sh", wrapper]
    run(*gradle_command, "--no-daemon", "assembleRelease", cwd=wrapper.parent, env=env)
    payload = ROOT / "module/play-integrity"
    if payload.exists():
        shutil.rmtree(payload)
    payload.mkdir(parents=True, exist_ok=True)
    shutil.copytree(ENGINE / "scripts", payload / "scripts", dirs_exist_ok=True)
    shutil.copy2(ROOT / f"target/pif-engine/{target}/release/pif-engine", payload / "pif-engine")
    dex = ENGINE / "android/app/build/intermediates/dex/release/minifyReleaseWithR8/classes.dex"
    shutil.copy2(dex, payload / "classes.dex")
    libraries = ENGINE / "android/app/build/intermediates/stripped_native_libs/release/stripReleaseDebugSymbols/out/lib"
    zygisk = ROOT / "module/zygisk"
    if zygisk.exists():
        shutil.rmtree(zygisk)
    zygisk.mkdir(exist_ok=True)
    for abi in ["arm64-v8a", "armeabi-v7a"]:
        shutil.copy2(libraries / abi / "libplayintegrityfix.so", zygisk / f"{abi}.so")
    notices = ROOT / "module/licenses/play-integrity"
    if notices.exists():
        shutil.rmtree(notices)
    notices.mkdir(parents=True, exist_ok=True)
    shutil.copy2(ENGINE / "LICENSE", notices / "GPL-3.0.txt")
    for name in ["README.md", "RULES.md", "NOTICE", "UPSTREAM.json"]:
        shutil.copy2(ENGINE / name, notices / name)
    for name in DEPENDENCIES:
        for path in (VENDOR / name).rglob("*"):
            if path.is_file() and ("LICENSE" in path.name.upper() or path.name.upper()=="NOTICE"):
                destination=notices / name / path.relative_to(VENDOR / name)
                destination.parent.mkdir(parents=True,exist_ok=True)
                shutil.copy2(path,destination)
    # Maven dependencies' license texts are kept with the source component.
    shutil.copytree(ENGINE / "licenses", notices, dirs_exist_ok=True)
    source_archive(notices.parent / "play-integrity-source.tar.gz")
    artifacts = [*(p for p in payload.rglob("*") if p.is_file()), *zygisk.glob("*.so")]
    (payload / "build.json").write_text(json.dumps({
        "upstream_revision": json.loads((ENGINE / "UPSTREAM.json").read_text())["revision"],
        "source_sha256": hashlib.sha256((notices.parent / "play-integrity-source.tar.gz").read_bytes()).hexdigest(),
        "files": {p.relative_to(ROOT / "module").as_posix(): hashlib.sha256(p.read_bytes()).hexdigest() for p in artifacts},
    }, indent=2) + "\n")


if __name__ == "__main__":
    main()
