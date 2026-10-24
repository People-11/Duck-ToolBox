import { afterEach, describe, expect, it } from "vitest"
import { setBridge } from "@/core/bridge"
import { deviceIdsApi, type DeviceIdsProfile, type DeviceIdsResult } from "./api"
import mock from "./mock"

const originalUrl = location.href
afterEach(() => {
  setBridge(null)
  history.replaceState(null, "", originalUrl)
})

function response(scenario: string, dryRun: boolean) {
  history.replaceState(null, "", `/?soc=${scenario}`)
  const profile = mock.handlers["device-ids.defaults"]!([], {}, undefined) as DeviceIdsProfile
  profile.dry_run = dryRun
  const data = mock.handlers["device-ids.provision"]!([], {}, profile) as DeviceIdsResult
  setBridge({
    available: true,
    exec: async () => ({
      errno: 0,
      stdout: JSON.stringify({ ok: true, api: 1, command: "device-ids.provision", data }),
      stderr: "",
    }),
    toast: () => undefined,
    hostModule: () => null,
    listPackages: async () => [],
    packagesInfo: async () => [],
    iconUrl: () => null,
    exit: () => undefined,
  })
  return { profile, data }
}

describe("device ID backend result contracts", () => {
  it.each([
    ["qualcomm", true],
    ["qualcomm", false],
    ["mediatek", true],
    ["mediatek", false],
  ] as const)("accepts %s with dry run %s", async (scenario, dryRun) => {
    const { profile, data } = response(scenario, dryRun)
    await expect(deviceIdsApi.provision(profile)).resolves.toEqual(data)
  })
  it.each([
    { loaded_library: "/vendor/lib64/libQSEEComAPI.so" },
    { verification: "ta_acknowledgement" },
    { ta_name: "keymaster" },
    { count: 99 },
    { executable: "/vendor/bin/kph" },
    { loaded_library: null },
  ])("rejects an inconsistent TrustKernel success report %j", async (invalid) => {
    const { profile, data } = response("mediatek", false)
    Object.assign(data, invalid)
    await expect(deviceIdsApi.provision(profile)).rejects.toMatchObject({
      code: "contract_mismatch",
    })
  })
})
