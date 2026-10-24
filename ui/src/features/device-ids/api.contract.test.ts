// @vitest-environment node
import { readFileSync } from "node:fs"
import { afterEach, describe, expect, it } from "vitest"

import type { Host } from "@/core/testing/host"
import { DEVICE_PROPS, CONTRACT, hasDuckd, installHost } from "@/core/testing/host"

import { deviceIdsApi } from "./api"

let host: Host | undefined
afterEach(() => host?.dispose())

describe.skipIf(!hasDuckd)("Device ID API against the real duckd", CONTRACT, () => {
  it("reads defaults from the device and runs a dry run", async () => {
    host = installHost({ props: DEVICE_PROPS })
    const defaults = await deviceIdsApi.defaults()
    expect(defaults.brand).toBe("google")
    expect(defaults.serial).toBe("39021FDH2000AB")
    expect(defaults.backend).toBe("qseecom")

    const result = await deviceIdsApi.provision({ ...defaults, dry_run: true })
    expect(result.dry_run).toBe(true)
    expect(result.loaded_library).toBeNull()
    expect(result.ids.map((id) => id.value)).toContain("google")
  })

  it("preserves MediaTek one-click defaults and previews both TrustKernel writes", async () => {
    host = installHost({
      props: {
        ...DEVICE_PROPS,
        "ro.soc.manufacturer": "MediaTek",
        "ro.hardware": "mt6789",
        "ro.vendor.mediatek.platform": "MT6789",
        "ro.serialno": "",
        "ro.boot.serialno": "",
      },
    })
    const defaults = await deviceIdsApi.defaults()
    expect(defaults).toMatchObject({
      backend: "trustkernel",
      ta_name: "keymaster",
      ta_path: "/vendor/app/t6",
    })
    const result = await deviceIdsApi.provision({
      ...defaults,
      meid: "A0000000002321",
      dry_run: true,
    })
    expect(result).toMatchObject({
      backend: "trustkernel",
      verification: "not_run",
      loaded_library: null,
      executable: null,
      ta_name: null,
      ta_path: null,
      omitted_ids: ["MEID"],
    })
    expect(result.ids.map((id) => id.label)).not.toContain("MEID")
    const report = JSON.parse(readFileSync(result.report_path, "utf8"))
    expect(report.request.items.map((item: { tag: string }) => item.tag)).toEqual([
      "rkp_flag",
      "attestation_ids",
    ])
    expect(Buffer.from(report.request.items[0].payload, "hex").at(-1)).toBe(0)
    expect(Buffer.from(report.request.items[1].payload, "hex").toString()).toContain("BRAND=google")
  })

  it("refuses conflicting SoC evidence before preparing a write", async () => {
    host = installHost({
      props: { ...DEVICE_PROPS, "ro.soc.manufacturer": "MediaTek", "ro.hardware": "qcom" },
    })
    const defaults = await deviceIdsApi.defaults()
    expect(defaults.backend).toBeNull()
    await expect(deviceIdsApi.provision({ ...defaults, dry_run: true })).rejects.toMatchObject({
      code: "unsupported_device",
    })
  })

  it("fills blank identifiers from the device, and refuses when the device has none", async () => {
    host = installHost({ props: DEVICE_PROPS })
    const defaults = await deviceIdsApi.defaults()
    const filled = await deviceIdsApi.provision({ ...defaults, brand: "", dry_run: true })
    expect(filled.ids.find((id) => id.label === "BRAND")?.value).toBe("google")
    host.dispose()

    const { "ro.product.brand": _brand, ...withoutBrand } = DEVICE_PROPS
    host = installHost({ props: withoutBrand })
    await expect(
      deviceIdsApi.provision({ ...defaults, brand: "", dry_run: true }),
    ).rejects.toMatchObject({ code: "missing_device_field" })
  })
})
