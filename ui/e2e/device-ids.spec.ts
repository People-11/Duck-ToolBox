import { expect, test } from "./fixtures"

test("clears a previous success when a later TrustKernel write fails", async ({ page, open }) => {
  await open("/device-ids", { query: { soc: "mediatek" } })
  await page.getByRole("button", { name: "Run dry run" }).click()
  await expect(page.getByText("device-ids-20260928.json")).toBeVisible()
  await page.evaluate(() => {
    const host = (
      window as unknown as {
        ksu: { exec: (shell: string, options: string, callback: string) => void }
      }
    ).ksu
    const original = host.exec
    host.exec = (shell, options, callback) => {
      if (!shell.includes("'device-ids' 'provision'")) {
        original(shell, options, callback)
        return
      }
      const response = {
        ok: false,
        api: 1,
        command: "device-ids.provision",
        data: null,
        error: {
          code: "trustkernel_operation_failed",
          message: "KPH import failed for attestation_ids; secure storage may have changed",
          details: {
            report_path: "/private/failed-write.json",
            backend: "trustkernel",
            writes: [
              { tag: "rkp_flag", import_succeeded: true, exists: null },
              { tag: "attestation_ids", import_succeeded: false, exists: null },
            ],
          },
        },
      }
      const done = (
        window as unknown as Record<string, (code: number, out: string, err: string) => void>
      )[callback]
      done!(1, JSON.stringify(response), "")
    }
  })
  await page.getByRole("switch", { name: "Dry run" }).click()
  await page.getByRole("button", { name: "Provision device IDs" }).click()
  await page.getByRole("alertdialog").getByRole("button", { name: "Provision device IDs" }).click()
  await expect(page.getByText(/KPH import failed for attestation_ids/)).toBeVisible()
  await expect(page.getByText("device-ids-20260928.json")).toBeHidden()
  await expect(page.getByRole("button", { name: "Provision device IDs" })).toBeEnabled()
  await page.getByRole("button", { name: "Details", exact: true }).click()
  await expect(page.getByText(/failed-write.json/)).toBeVisible()
})
