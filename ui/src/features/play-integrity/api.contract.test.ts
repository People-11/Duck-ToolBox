// @vitest-environment node
import { duckd } from "@/core/duckd"
import { z } from "zod"
import { existsSync, readFileSync, writeFileSync } from "node:fs"
import path from "node:path"
import { afterEach, describe, expect, it } from "vitest"
import { CONTRACT, hasDuckd, installHost } from "@/core/testing/host"
import type { Host } from "@/core/testing/host"
import { integrityApi } from "./api"
import { libraryApi } from "./library-api"

const repository = path.resolve(import.meta.dirname, "../../../..")
const engine =
  process.env.PIF_ENGINE_BIN ?? path.join(repository, "target/pif-engine/debug/pif-engine")
if (process.env.DUCKD_REQUIRED === "1" && !existsSync(engine))
  throw new Error("contract tests require the independently built pif-engine")
let host: Host | undefined
afterEach(() => host?.dispose())
describe.skipIf(!hasDuckd || !existsSync(engine))(
  "Play Integrity adapter against the standalone engine",
  CONTRACT,
  () => {
    it("stores snapshots and manual results through the engine without applying or enabling", async () => {
      host = installHost()
      await integrityApi.disable()
      const before = await integrityApi.config()
      const snapshot = await libraryApi.saveProfile(
        "Contract snapshot",
        "MODEL=Snapshot\nCUSTOM=retain",
      )
      const id = snapshot.profiles[0]!.id
      const preview = await libraryApi.profilePreview(id)
      expect(preview.entries.CUSTOM).toBe("retain")
      expect(await integrityApi.config()).toEqual(before)
      expect((await integrityApi.power()).enabled).toBe(false)
      expect((await libraryApi.inspect()).rows.every((row) => row.observed === null)).toBe(true)
      const records = await libraryApi.saveRecord({
        app: "Manual fixture",
        version: "1",
        note: "",
        basic: "pass",
        device: "unknown",
        strong: "fail",
        revision: before.revision,
      })
      expect(records.user_reported).toBe(true)
      expect(records.verdict_verified).toBe(false)
      expect(records.records[0]?.enabled).toBe(false)
      expect((await libraryApi.records()).records).toHaveLength(1)
      await libraryApi.clearRecords()
      expect((await libraryApi.records()).records).toHaveLength(0)
      await libraryApi.deleteProfile(id)
      expect((await libraryApi.profiles()).profiles).toHaveLength(0)
      const special = await integrityApi.preview(
        JSON.stringify({ MODEL: "Draft", "custom.hash": "a#b" }),
      )
      expect(special.content.trimStart().startsWith("{")).toBe(true)
      expect(special.entries["custom.hash"]).toBe("a#b")
      expect(await integrityApi.config()).toEqual(before)
    })
    it("preserves the independent engine protocol across preview, apply and restore", async () => {
      host = installHost()
      const status = await integrityApi.status()
      expect(status.options.enabled).toBe(true)
      expect(status.verdict_verified).toBe(false)
      const before = await integrityApi.config()
      const preview = await integrityApi.preview(
        "MODEL=Contract\nEXTRA=preserved\nSECURITY_PATCH=2026-10-05",
        "prop",
        true,
      )
      expect((await integrityApi.config()).revision).toBe(before.revision)
      const saved = await integrityApi.apply(preview)
      expect(saved.config.entries.EXTRA).toBe("preserved")
      await expect(integrityApi.apply(preview)).rejects.toMatchObject({ code: "engine_error" })
      expect((await integrityApi.restore(saved.backup)).config.entries).toEqual({})
    })
    it("persists interrupted cross-module work and requires confirmed, repeatable recovery", async () => {
      host = installHost({
        files: {
          "/data/adb/modules/oh_my_keymint/module.prop": "id=oh_my_keymint\nversionCode=1",
          "/data/misc/keystore/omk/config.toml": '[trust]\nboot_patchlevel="2026-09-05"\n',
        },
      })
      const preview = await integrityApi.preview("MODEL=Journal\nSECURITY_PATCH=2026-10-05")
      const patch = await duckd(["tricky-store", "patch", "preview"], {
        input: { patch: "2026-10-05" },
        schema: z.object({ patch: z.string(), revision: z.string() }),
      })
      await integrityApi.applyTransaction(preview, { target: "tricky-store", ...patch })
      const status = await integrityApi.transactionStatus()
      expect(status.pending).toBe(false)
      const journal = JSON.parse(readFileSync(status.journal_path, "utf8"))
      // Reproduce termination after both writes but before the completion record.
      journal.phase = "applying"
      writeFileSync(status.journal_path, JSON.stringify(journal))
      expect((await integrityApi.transactionStatus()).pending).toBe(true)
      await expect(
        integrityApi.applyTransaction(await integrityApi.preview("MODEL=Blocked")),
      ).rejects.toThrow()
      await expect(
        duckd(["integrity-transaction", "recover"], {
          input: { confirmed: false },
          schema: z.unknown(),
        }),
      ).rejects.toThrow()
      expect((await integrityApi.recoverTransaction()).restart_required).toBe(true)
      expect((await integrityApi.transactionStatus()).pending).toBe(false)
      expect((await integrityApi.config()).entries).toEqual({})
      expect(readFileSync(host.path("/data/misc/keystore/omk/config.toml"), "utf8")).toBe(
        '[trust]\nboot_patchlevel="2026-09-05"\n',
      )
    })
    it("disables with broken configuration, keeps saved files, and requires a fresh power revision to enable", async () => {
      host = installHost({
        files: {
          "/data/adb/play-integrity/options.json": "{broken",
          "/data/adb/play-integrity/custom.pif.json": "{broken config",
          "/data/adb/play-integrity/custom.app_replace_list.txt": "/system/",
        },
      })
      const before = await integrityApi.power()
      const stopped = await integrityApi.disable()
      expect(stopped.enabled).toBe(false)
      expect((await integrityApi.power()).enabled).toBe(false)
      expect(readFileSync(host.path("/data/adb/play-integrity/custom.pif.json"), "utf8")).toBe(
        "{broken config",
      )
      writeFileSync(host.path("/data/adb/play-integrity/options.json"), "{}")
      await expect(
        integrityApi.options({ enabled: true, expected_stop_revision: before.stop_revision }),
      ).rejects.toThrow()
      await integrityApi.options({ enabled: true, expected_stop_revision: stopped.stop_revision })
      expect((await integrityApi.power()).enabled).toBe(true)
      await integrityApi.options({ enabled: false, app_replace_list: "/system/" })
      expect((await integrityApi.power()).enabled).toBe(false)
    })
    it("imports without changing the source and preserves conflict state", async () => {
      host = installHost({
        files: {
          "/data/adb/modules/playintegrityfix/module.prop": "id=playintegrityfix",
          "/data/local/tmp/custom.pif.prop": "MODEL=Imported\nEXTRA=keep",
        },
      })
      expect((await integrityApi.status()).conflicts).toEqual(["playintegrityfix"])
      const preview = await integrityApi.importPreview("/data/local/tmp/custom.pif.prop")
      expect(preview.entries.EXTRA).toBe("keep")
      expect((await integrityApi.config()).entries).toEqual({})
    })
    it("exposes the reviewed API and Provider rules without rewriting explicit choices", async () => {
      host = installHost()
      const empty = await integrityApi.preview("MODEL=New", "prop", true)
      expect(empty.entries.spoofProvider).toBe("0")
      expect(empty.entries.DEVICE_INITIAL_SDK_INT).toBeUndefined()
      expect(empty.entries["ro.board.first_api_level"]).toBeUndefined()
      expect(empty.warnings.some((warning) => warning.code === "missing_api")).toBe(true)
      const explicit = await integrityApi.preview(
        "MODEL=A\nro.product.model=B\nDEVICE_INITIAL_SDK_INT=36\nro.board.first_api_level=35\nspoofProvider=1",
        "prop",
        true,
      )
      expect(explicit.entries.DEVICE_INITIAL_SDK_INT).toBe("36")
      expect(explicit.entries["ro.board.first_api_level"]).toBe("35")
      expect(explicit.entries.spoofProvider).toBe("1")
      expect(explicit.warnings.some((warning) => warning.code === "mismatch")).toBe(true)
    })
  },
)
