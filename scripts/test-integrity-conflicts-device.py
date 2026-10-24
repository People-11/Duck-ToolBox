#!/usr/bin/env python3
"""Exercise installed PI/OMK configuration contracts on a disposable rooted AVD."""
import argparse
import json
import subprocess
import time
import tomllib
import unittest

MODULE = "/data/adb/modules/duck-toolbox"
PI_DATA = "/data/adb/play-integrity"
OMK = "/data/misc/keystore/omk"


class Conflicts(unittest.TestCase):
    serial = ""

    def shell(self, command, payload=None):
        result = subprocess.run(
            ["adb", "-s", self.serial, "shell", command], input=payload,
            capture_output=True, timeout=60,
        )
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
        return result.stdout

    def request(self, feature, operation, payload=None, ok=True):
        command = f"{MODULE}/bin/duckctl.sh {feature} {operation}"
        if payload is not None:
            command += " --stdin-json"
        result = subprocess.run(
            ["adb", "-s", self.serial, "shell", command],
            input=None if payload is None else json.dumps(payload).encode(),
            capture_output=True, timeout=60,
        )
        envelope = json.loads(result.stdout)
        self.assertEqual(envelope["ok"], ok, envelope.get("error"))
        return envelope["data"] if ok else envelope["error"]

    def status(self):
        return self.request("tricky-store", "status")

    def trust(self):
        return tomllib.loads(self.shell(f"cat {OMK}/config.toml").decode())["trust"]

    def setUp(self):
        self.assertTrue(self.serial.startswith("emulator-"), "Use a disposable AVD")
        self.assertEqual(self.shell("id -u").strip(), b"0")
        status = self.status()
        self.assertEqual(status["active"]["backend"], "oh-my-keymint")
        self.assertTrue(status["active"]["active"])
        self.assertEqual(status["active"]["version"], "1.2.0-b12557c")
        paths = [
            "/data/adb/boot_hash", "/data/adb/disable_prop_handler",
            f"{OMK}/config.toml", f"{OMK}/injector.toml",
            *[f"{PI_DATA}/{name}" for name in
              ["custom.pif.json", "options.json", "disabled", "enabled-token"]],
        ]
        saved = {}
        for path in paths:
            exists = self.shell(f"if [ -f {path} ]; then printf yes; fi") == b"yes"
            saved[path] = self.shell(f"cat {path}") if exists else None

        def restore():
            for path, content in saved.items():
                if content is None:
                    self.shell(f"rm -f {path}")
                else:
                    self.shell(f"cat > {path}", content)
        self.addCleanup(restore)

    def test_settings_and_patch_transactions_preserve_independent_owners(self):
        initial_props = self.status()["props"]
        hash_value = "ab" * 32
        props = {"prop_handler_enabled": False, "boot_hash": hash_value}
        result = self.request("tricky-store", "props", props)
        self.assertTrue(result["applied_boot_hash"])
        self.assertTrue(result["synced_backend_policy"])
        self.assertTrue(result["restart_required"])
        self.assertEqual(self.shell("getprop ro.boot.vbmeta.digest").strip().decode(), hash_value)
        self.assertEqual(self.trust()["vb_hash"], hash_value)
        self.request("tricky-store", "props", {
            "prop_handler_enabled": True, "boot_hash": None, "expected": initial_props,
        }, ok=False)
        self.assertEqual(self.status()["props"]["boot_hash"], hash_value)

        # A separately edited backend policy must survive a handler-only save.
        status = self.status()
        config = status["config"]
        request = {**config, "system_apps": status["system_apps"],
                   "auto_add_new_apps": status["auto_add_new_apps"],
                   "expected_config": config}
        request["default_policy"] = {**config["default_policy"], "vb_hash": "random"}
        self.request("tricky-store", "save", request)
        props["prop_handler_enabled"] = True
        result = self.request("tricky-store", "props", props)
        self.assertFalse(result["synced_backend_policy"])
        self.assertEqual(self.trust()["vb_hash"], "random")
        stale = self.status()

        # Only matching, enabled Build/property values may recommend a patch.
        preview = self.request("play-integrity", "preview", {
            "content": "MODEL=Fixture\nSECURITY_PATCH=2026-10-05\n"
                       "ro.build.version.security_patch=2026-10-05\nspoofProvider=0",
        })
        self.assertEqual(preview["suggested_patch"], "2026-10-05")
        patch = self.request("tricky-store", "patch preview", {"patch": "2026-10-05"})
        applied = self.request("integrity-transaction", "apply", {
            "engine": preview, "patch": {"target": "tricky-store", **patch},
        })
        self.assertTrue(applied["restart_required"])
        trust = self.trust()
        for key in ["security_patch", "os_patchlevel", "vendor_patchlevel", "boot_patchlevel"]:
            self.assertEqual(trust[key], "2026-10-05")
        self.assertEqual(trust["vb_hash"], "random")
        error = self.request("tricky-store", "save", {
            **stale["config"], "expected_config": stale["config"],
        }, ok=False)
        self.assertIn("configuration changed", error["message"])
        self.assertEqual(self.trust(), trust)

        # OMK accepts only a 32-byte hash: failed validation changes no shared files.
        before = self.status()["props"]
        self.request("tricky-store", "props", {
            "prop_handler_enabled": False, "boot_hash": "cd" * 64,
        }, ok=False)
        self.assertEqual(self.status()["props"], before)
        self.assertEqual(self.trust(), trust)
        for content in [
            "SECURITY_PATCH=2026-10-05\n*.security_patch=2026-09-05",
            "SECURITY_PATCH=2026-10-05\nspoofBuild=0\nspoofProps=0",
            "SECURITY_PATCH=2026-02-29",
        ]:
            result = self.request("play-integrity", "preview", {"content": content})
            self.assertIsNone(result["suggested_patch"])

        # Stop PI independently, then verify both backend configuration and daemon.
        self.request("play-integrity", "disable")
        self.assertEqual(self.request("play-integrity", "status")["state"], "disabled")
        self.assertTrue(self.status()["active"]["active"])
        self.assertEqual(self.trust(), trust)
        self.assertTrue(self.shell("pidof keymint").strip())
        time.sleep(1)
        logs = self.shell("logcat -d -s OhMyKeymint:V").decode(errors="replace")
        self.assertNotIn("failed to parse config", logs)
        self.assertNotIn("moved invalid config", logs)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", required=True, help="Disposable emulator-NNNN serial")
    args = parser.parse_args()
    Conflicts.serial = args.serial
    unittest.main(argv=[__file__])
