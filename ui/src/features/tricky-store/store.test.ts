import { createPinia, setActivePinia } from "pinia"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import { trickyStoreApi } from "./api"
import type { Saved, Status } from "./api"
import { draftFromStatus, sameDraft } from "./selection"
import { useTrickyStore } from "./store"

const status: Status = {
  backends: [],
  active: null,
  schema: null,
  config_error: null,
  keybox: null,
  config: {
    targets: [{ package_name: "com.saved", mode: "auto" }],
    default_policy: { os_patch: "prop" },
    per_app_policy: {},
  },
  packages: [],
  system_apps: [],
  auto_add_new_apps: false,
  props: { prop_handler_enabled: true, boot_hash: null },
  root_manager: null,
}
const saved: Saved = { target_count: 1, restart_required: false }

beforeEach(() => setActivePinia(createPinia()))
afterEach(() => vi.restoreAllMocks())

describe("target saves", () => {
  it("retains the configuration read before a competing policy update", async () => {
    const write = vi.spyOn(trickyStoreApi, "save").mockResolvedValue(saved)
    const store = useTrickyStore()
    store.load(status)
    store.toggle("com.pending")
    store.load({ ...status, config: { ...status.config, default_policy: { os_patch: "202610" } } })
    await store.save()
    expect(write.mock.calls[0]![0].expected_config).toEqual(status.config)
    expect(write.mock.calls[0]![0].default_policy).toEqual({ os_patch: "prop" })
  })
  it("retains the clean draft when a page reattaches to the same cached status", () => {
    const store = useTrickyStore()
    store.load(status)
    const draft = store.draft
    store.load(status)
    expect(store.draft).toBe(draft)
    store.load(status, true)
    expect(store.draft).not.toBe(draft)
  })

  it("advances the baseline without losing edits made during the write", async () => {
    let finish!: (result: Saved) => void
    vi.spyOn(trickyStoreApi, "save").mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve
        }),
    )
    const store = useTrickyStore()
    store.load(status)
    store.toggle("com.first")
    const pending = store.save()
    expect(store.saving).toBe(true)
    store.toggle("com.second")
    finish(saved)
    await pending
    expect(store.status?.config.targets.map((target) => target.package_name)).toEqual([
      "com.first",
      "com.saved",
    ])
    expect(store.draft?.targets.has("com.second")).toBe(true)
    expect(store.dirty).toBe(true)
    store.load(store.status!)
    expect(store.draft?.targets.has("com.second")).toBe(true)
    expect(store.saving).toBe(false)
  })

  it("keeps failed writes dirty and releases the save guard", async () => {
    vi.spyOn(trickyStoreApi, "save").mockRejectedValue(new Error("failed"))
    const store = useTrickyStore()
    store.load(status)
    store.toggle("com.new")
    await expect(store.save()).rejects.toThrow("failed")
    expect(store.dirty).toBe(true)
    expect(store.status).toEqual(status)
    expect(store.saving).toBe(false)
  })

  it("blocks overlapping writes and saves settings without flushing target edits", async () => {
    let finish!: (result: Saved) => void
    const write = vi.spyOn(trickyStoreApi, "save").mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve
        }),
    )
    const store = useTrickyStore()
    store.load(status)
    store.toggle("com.pending")
    const pending = store.saveOnly({ autoAdd: true })
    await expect(store.save()).rejects.toMatchObject({ code: "operation_pending" })
    finish(saved)
    await pending
    expect(write).toHaveBeenCalledTimes(1)
    expect(store.status?.config.targets).toEqual(status.config.targets)
    expect(store.draft?.targets.has("com.pending")).toBe(true)
    expect(store.draft?.autoAdd).toBe(true)
    expect(store.dirty).toBe(true)
  })

  it("takes an immutable policy snapshot and preserves subsequent policy edits", async () => {
    let finish!: (result: Saved) => void
    vi.spyOn(trickyStoreApi, "save").mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve
        }),
    )
    const store = useTrickyStore()
    store.load(status)
    store.setMode("com.saved", "generate", { os_patch: "202601" })
    const pending = store.save()
    store.draft!.perApp.get("com.saved")!.os_patch = "202602"
    finish(saved)
    await pending
    expect(store.status?.config.per_app_policy["com.saved"]).toEqual({ os_patch: "202601" })
    expect(store.dirty).toBe(true)
  })
})

describe("draft equality", () => {
  it("ignores order and untargeted policies while noticing saved field changes", () => {
    const first = draftFromStatus(status)
    const second = draftFromStatus(status)
    first.perApp.set("com.unselected", { ignored: "value" })
    expect(sameDraft(first, second)).toBe(true)
    first.defaultPolicy = { a: "1", b: "2" }
    second.defaultPolicy = { b: "2", a: "1" }
    expect(sameDraft(first, second)).toBe(true)
    first.targets.set("com.saved", "hack")
    expect(sameDraft(first, second)).toBe(false)
  })
})
