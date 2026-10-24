import { expect, test } from "./fixtures"

const CLI = "/data/adb/modules/duck-toolbox/bin/duckctl.sh"

test("@oh_my_keymint saves prop settings through the native WebUI bridge", async ({
  page,
  device,
}) => {
  const request = async (operation: string, input?: unknown) => {
    const json = JSON.stringify(input ?? null).replaceAll("'", "'\\''")
    const command =
      input === undefined
        ? `${CLI} ${operation}`
        : `printf '%s' '${json}' | ${CLI} ${operation} --stdin-json`
    const result = JSON.parse(await device.shell(command)) as {
      ok: boolean
      data: {
        props: { prop_handler_enabled: boolean; boot_hash: string | null }
        config: {
          targets: unknown[]
          default_policy: Record<string, string>
          per_app_policy: unknown
        }
        system_apps: string[]
        auto_add_new_apps: boolean
      }
    }
    expect(result.ok).toBe(true)
    return result.data
  }
  const original = await request("tricky-store status")
  try {
    await page.evaluate(() => (location.hash = "#/tricky-store/props"))
    const hash = page.getByLabel("Boot Hash")
    await expect(hash).toBeVisible()
    await hash.fill("CD".repeat(32))
    const handler = page.getByRole("switch")
    if ((await handler.getAttribute("aria-checked")) === "true") await handler.click()
    await page.getByRole("button", { name: "Save", exact: true }).click()
    await expect(page.getByText("Verified Boot Hash saved successfully")).toBeVisible()
    await expect(hash).toHaveValue("cd".repeat(32))
    expect((await device.shell("cat /data/adb/boot_hash")).trim()).toBe("cd".repeat(32))
    expect((await device.shell("getprop ro.boot.vbmeta.digest")).trim()).toBe("cd".repeat(32))
    expect(await device.shell("test -f /data/adb/disable_prop_handler && printf disabled")).toBe(
      "disabled",
    )
    const status = await request("tricky-store status")
    expect(status.config.default_policy.vb_hash).toBe("cd".repeat(32))
    await hash.fill("")
    await page.getByRole("button", { name: "Save", exact: true }).click()
    await expect(page.getByText("Config saved", { exact: true })).toBeVisible()
    expect(await device.shell("test ! -f /data/adb/boot_hash && printf cleared")).toBe("cleared")
    expect((await request("tricky-store status")).config.default_policy.vb_hash).toBe(
      "cd".repeat(32),
    )
    await device.screenshot("integrity-props-omk")
    await hash.fill("EF".repeat(32))
    await request("tricky-store props", {
      prop_handler_enabled: false,
      boot_hash: "12".repeat(32),
    })
    await page.getByRole("button", { name: "Save", exact: true }).click()
    await expect(page.getByText(/prop settings changed/)).toBeVisible()
    expect((await device.shell("cat /data/adb/boot_hash")).trim()).toBe("12".repeat(32))
    await page.getByRole("button", { name: "Refresh", exact: true }).click()
    await expect(hash).toHaveValue("12".repeat(32))
    await page.getByRole("button", { name: "Save", exact: true }).click()
    await expect(page.getByText("Verified Boot Hash saved successfully").last()).toBeVisible()
  } finally {
    await request("tricky-store props", original.props)
    const current = await request("tricky-store status")
    await request("tricky-store save", {
      ...original.config,
      expected_config: current.config,
      system_apps: original.system_apps,
      auto_add_new_apps: original.auto_add_new_apps,
    })
  }
})
