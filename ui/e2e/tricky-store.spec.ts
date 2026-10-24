import { readFileSync } from "node:fs"

import { expect, test } from "./fixtures"

const row = (name: string) => ({ role: "checkbox" as const, name: new RegExp(name) })

test.describe("target list", () => {
  test("selects an app and saves", async ({ page, open }) => {
    await open("/tricky-store")
    await expect(page.getByText("4 selected")).toBeVisible()
    await page.getByRole(row("Discord").role, { name: row("Discord").name }).click()
    await expect(page.getByText("5 selected")).toBeVisible()
    await page.getByRole("button", { name: "Save", exact: false }).last().click()
    await expect(page.getByText("Config saved")).toBeVisible()
  })

  test("sets a per-app mode from the mode sheet", async ({ page, open }) => {
    await open("/tricky-store")
    await page.getByRole("button", { name: "Mode and policy of Key Attestation" }).click()
    const sheet = page.getByRole("dialog")
    await expect(sheet.getByText("Mode", { exact: true })).toBeVisible()
    await sheet.getByLabel("Leaf Hack").check()
    await sheet.getByRole("button", { name: "Save" }).click()
    await expect(sheet).toBeHidden()
    await expect(
      page.getByRole("checkbox", { name: /Key Attestation/ }).getByText("Leaf Hack"),
    ).toBeVisible()
  })

  test("closes the mode sheet with the back gesture", async ({ page, open }) => {
    await open("/tricky-store")
    await page.getByRole("button", { name: "Mode and policy of Google Wallet" }).click()
    await expect(page.getByRole("dialog")).toBeVisible()
    await page.goBack()
    await expect(page.getByRole("dialog")).toBeHidden()
    await expect(page).toHaveURL(/#\/tricky-store$/)
  })

  test("filters by label or package name", async ({ page, open }) => {
    await open("/tricky-store")
    await page.getByPlaceholder("Search").fill("wallet")
    await expect(page.getByRole("checkbox", { name: /Google Wallet/ })).toBeVisible()
    await expect(page.getByRole("checkbox", { name: /Discord/ })).toBeHidden()
  })

  test("select all, then deselect unnecessary apps", async ({ page, open }) => {
    await open("/tricky-store")
    await page.getByRole("button", { name: "More" }).click()
    await page.getByRole("menuitem", { name: "Select all", exact: true }).click()
    await page.getByRole("button", { name: "More" }).click()
    await page.getByRole("menuitem", { name: "Deselect Unnecessary" }).click()
    await expect(page.getByText("4 apps deselected")).toBeVisible()
    await expect(page.getByRole("checkbox", { name: /LSPosed/ })).toHaveAttribute(
      "aria-checked",
      "false",
    )
  })

  test("offers the DenyList only on Magisk", async ({ page, open }) => {
    await open("/tricky-store", { query: { manager: "magisk" } })
    await page.getByRole("button", { name: "More" }).click()
    await page.getByRole("menuitem", { name: "Select From DenyList" }).click()
    await expect(page.getByText("2 apps selected")).toBeVisible()
  })

  test("adds a system app", async ({ page, open }) => {
    await open("/tricky-store")
    await expect(page.getByRole("checkbox", { name: /Chrome/ })).toBeHidden()
    await page.getByRole("button", { name: "More" }).click()
    await page.getByRole("menuitem", { name: "Add System App" }).click()
    const dialog = page.getByRole("dialog")
    await dialog.getByRole("checkbox", { name: /Chrome/ }).click()
    await dialog.getByRole("button", { name: "Save" }).click()
    await expect(page.getByRole("checkbox", { name: /Chrome/ })).toHaveAttribute(
      "aria-checked",
      "true",
    )
  })
})

test.describe("keybox", () => {
  test("rejects oversized picker uploads and allows selecting another file", async ({
    page,
    open,
  }) => {
    await open("/tricky-store/keybox")
    await page.getByRole("button", { name: "Import", exact: true }).click()
    const dialog = page.getByRole("dialog")
    const input = dialog.locator('input[type="file"]')
    await input.setInputFiles({
      name: "large.json",
      mimeType: "application/json",
      buffer: Buffer.alloc(65 * 1024, " "),
    })
    await expect(page.getByText(/Upload limit:/)).toBeVisible()
    await expect(dialog).toBeVisible()
    await input.setInputFiles({
      name: "providers.json",
      mimeType: "application/json",
      buffer: Buffer.from("[]"),
    })
    await expect(dialog).toBeHidden()
    await expect(page.getByText("Config imported", { exact: true })).toBeVisible()
  })

  test("ignores a picker read completed after cancellation", async ({ page, open }) => {
    await open("/tricky-store/keybox")
    await page.getByRole("button", { name: "Import", exact: true }).click()
    await page.evaluate(() => {
      const scope = window as unknown as Record<string, unknown>
      const host = scope.ksu as { exec: (shell: string, options: string, callback: string) => void }
      const original = host.exec
      scope.importCalls = 0
      host.exec = (shell, options, callback) => {
        if (shell.includes("'providers' 'import-content'"))
          scope.importCalls = Number(scope.importCalls) + 1
        original(shell, options, callback)
      }
      File.prototype.text = () =>
        new Promise<string>((resolve) => {
          scope.finishRead = resolve
        })
    })
    const dialog = page.getByRole("dialog")
    await dialog.locator('input[type="file"]').setInputFiles({
      name: "providers.json",
      mimeType: "application/json",
      buffer: Buffer.from("[]"),
    })
    await expect(dialog.getByRole("button", { name: "System file picker" })).toBeDisabled()
    await dialog.getByRole("button", { name: "Cancel" }).click()
    const calls = await page.evaluate(async () => {
      const scope = window as unknown as {
        finishRead: (content: string) => void
        importCalls: number
      }
      scope.finishRead("[]")
      await new Promise((resolve) => setTimeout(resolve, 0))
      return scope.importCalls
    })
    expect(calls).toBe(0)
  })

  test("installs the AOSP keybox", async ({ page, open }) => {
    await open("/tricky-store/keybox")
    await page.getByRole("button", { name: /AOSP/ }).click()
    await expect(page.getByText("AOSP keybox set successfully")).toBeVisible()
  })

  test("warns before OMK provider installs and lets the user cancel", async ({ page, open }) => {
    await open("/tricky-store/keybox", { query: { backend: "oh-my-keymint" } })
    await page.getByRole("button", { name: "Example provider", exact: true }).click()
    const dialog = page.getByRole("alertdialog")
    await expect(dialog.getByText("OMK keybox compatibility", { exact: true })).toBeVisible()
    await expect(dialog.getByText(/b12557c/)).toBeVisible()
    await expect(dialog).toHaveCSS("opacity", "1")
    await page.screenshot({ path: "test-results/omk-keybox-warning.png", animations: "disabled" })
    await dialog.getByRole("button", { name: "Cancel", exact: true }).click()
    await expect(page.getByText("Custom keybox set successfully", { exact: true })).toBeHidden()
    await page.getByRole("button", { name: "Example provider", exact: true }).click()
    await dialog.getByRole("button", { name: "Continue installation", exact: true }).click()
    await expect(page.getByText("Custom keybox set successfully", { exact: true })).toBeVisible()
  })

  test("warns before OMK browser imports but installs bundled keyboxes directly", async ({
    page,
    open,
  }) => {
    await open("/tricky-store/keybox", { query: { backend: "oh-my-keymint" } })
    await page.getByRole("button", { name: /AOSP/ }).click()
    await expect(page.getByText("AOSP keybox set successfully")).toBeVisible()
    await expect(page.getByRole("alertdialog")).toBeHidden()
    await page.getByRole("button", { name: /Local/ }).click()
    await page
      .getByRole("dialog")
      .locator('input[type="file"]')
      .setInputFiles({
        name: "ec-only.xml",
        mimeType: "application/xml",
        buffer: Buffer.from(
          readFileSync(
            new URL("../../crates/duck-tricky-store/assets/aosp-keybox.xml", import.meta.url),
            "utf8",
          ).replace(/\s*<Key algorithm="rsa">[\s\S]*?<\/Key>/, ""),
        ),
      })
    const dialog = page.getByRole("alertdialog")
    await expect(dialog.getByText("OMK keybox compatibility", { exact: true })).toBeVisible()
    await dialog.getByRole("button", { name: "Continue installation", exact: true }).click()
    await expect(page.getByText("Custom keybox set successfully", { exact: true })).toBeVisible()
  })

  test("rejects a provider with a disallowed decode step", async ({ page, open }) => {
    await open("/tricky-store/keybox")
    await page.getByRole("button", { name: "Add provider" }).click()
    const dialog = page.getByRole("dialog")
    await dialog.getByLabel("Name").fill("Mine")
    await dialog.getByLabel("URL").fill("https://example.org/kb")
    await dialog.getByLabel("Decode script").fill("sh -c reboot")
    await dialog.getByRole("button", { name: "Save" }).click()
    await expect(page.getByText("Decode step not allowed")).toBeVisible()
  })

  test("reports a failed provider download with Tricky Addon's message", async ({ page, open }) => {
    await open("/tricky-store/keybox")
    await page.getByRole("button", { name: "Add provider" }).click()
    const dialog = page.getByRole("dialog")
    await dialog.getByLabel("Name").fill("Broken")
    await dialog.getByLabel("URL").fill("https://example.org/fail")
    await dialog.getByRole("button", { name: "Save" }).click()
    await page.getByRole("button", { name: "Broken" }).click()
    await expect(page.getByText("Failed to fetch keybox")).toBeVisible()
  })
})

test.describe("keybox repository", () => {
  // A stand-in for keybox.kowx712.cc that speaks Tricky Addon's repo-api.md protocol.
  const site = (reply: string) => `<!doctype html><script>
    addEventListener("message", (event) => {
      if (event.data?.type !== "handshake") return
      event.source.postMessage({ type: "handshake_ack" }, event.origin)
      setTimeout(() => event.source.postMessage(${reply}, event.origin), 100)
    })
  </script>`

  test("installs the keybox the site hands over after the handshake", async ({ page, open }) => {
    await page.route("https://keybox.kowx712.cc/**", (route) =>
      route.fulfill({
        contentType: "text/html",
        body: site(`{ type: "download", url: "https://keybox.kowx712.cc/api/kb/1" }`),
      }),
    )
    await open("/tricky-store/keybox/repo")
    await expect(page.getByText("Keybox set from repo successfully")).toBeVisible()
    await expect(page).toHaveURL(/#\/tricky-store\/keybox$/)
  })

  test("keeps the OMK compatibility dialog open until the repository install is confirmed", async ({
    page,
    open,
  }) => {
    await page.route("https://keybox.kowx712.cc/**", (route) =>
      route.fulfill({
        contentType: "text/html",
        body: site(`{ type: "download", url: "https://keybox.kowx712.cc/api/kb/1" }`),
      }),
    )
    await open("/tricky-store/keybox/repo", { query: { backend: "oh-my-keymint" } })
    const dialog = page.getByRole("alertdialog")
    await expect(dialog.getByText("OMK keybox compatibility", { exact: true })).toBeVisible()
    await expect(page).toHaveURL(/#\/tricky-store\/keybox\/repo$/)
    await dialog.getByRole("button", { name: "Continue installation", exact: true }).click()
    await expect(page.getByText("Keybox set from repo successfully")).toBeVisible()
    await expect(page).toHaveURL(/#\/tricky-store\/keybox$/)
  })

  test("reports a download the site could not serve", async ({ page, open }) => {
    await page.route("https://keybox.kowx712.cc/**", (route) =>
      route.fulfill({
        contentType: "text/html",
        body: site(`{ type: "error", error: "download_failed", identity: "Pixel-42" }`),
      }),
    )
    await open("/tricky-store/keybox/repo")
    await expect(page.getByText("Download failed for Pixel-42")).toBeVisible()
  })

  test("returns to keybox management when a repository keybox fails to download", async ({
    page,
    open,
  }) => {
    await page.route("https://keybox.kowx712.cc/**", (route) =>
      route.fulfill({
        contentType: "text/html",
        body: site(`{ type: "download", url: "https://keybox.kowx712.cc/api/fail" }`),
      }),
    )
    await open("/tricky-store/keybox/repo", { query: { backend: "oh-my-keymint" } })
    await page
      .getByRole("alertdialog")
      .getByRole("button", { name: "Continue installation", exact: true })
      .click()
    await expect(page.getByText("Failed to set keybox from repo", { exact: true })).toBeVisible()
    await expect(page).toHaveURL(/#\/tricky-store\/keybox$/)
  })
})

test.describe("backends", () => {
  test("OhMyKeymint has no per-app modes but a boolean policy", async ({ page, open }) => {
    await open("/tricky-store", { query: { backend: "oh-my-keymint" } })
    await expect(page.getByText("OMK", { exact: true })).toBeVisible()
    await expect(page.getByRole("button", { name: /Mode and policy of/ })).toHaveCount(0)
    await open("/tricky-store/policy", { query: { backend: "oh-my-keymint" } })
    await expect(page.getByRole("switch", { name: "Bootloader locked" })).toBeVisible()
  })

  test("says when the module in use is disabled", async ({ page, open }) => {
    await open("/tricky-store", { query: { disabled: "1" } })
    await expect(page.getByText(/is disabled in your root manager/)).toBeVisible()
    await expect(page.locator("main").getByText("Tricky Store", { exact: true })).toHaveCount(1)
  })

  test("explains what to install when no keystore module exists", async ({ page, open }) => {
    await open("/tricky-store", { query: { backend: "none" } })
    await expect(page.getByText("No keystore module found")).toBeVisible()
  })

  test("opens straight into the manager from a keystore module's WebUI", async ({ page, open }) => {
    await open("/", { query: { host: "tricky_store" } })
    await expect(page).toHaveURL(/#\/tricky-store$/)
  })
})

test("saves prop settings", async ({ page, open }) => {
  await open("/tricky-store/props")
  await expect(page.getByLabel("Boot Hash")).toHaveValue("")
  await expect(page.getByLabel("Boot Hash")).toHaveAttribute(
    "placeholder",
    "Enter 64 / 128 hexadecimal characters",
  )
  await page.getByLabel("Boot Hash").fill("zz")
  await expect(page.getByText("Enter 64 / 128 hexadecimal characters")).toBeVisible()
  await page.getByLabel("Boot Hash").fill(`${"AB ".repeat(16)}\n${"AB".repeat(16)}`)
  await page.getByRole("button", { name: "Save" }).click()
  await expect(page.getByText("Verified Boot Hash saved successfully")).toBeVisible()
  await expect(page.getByLabel("Boot Hash")).toHaveValue("ab".repeat(32))
  await page.getByLabel("Boot Hash").fill("")
  await page.getByRole("button", { name: "Save" }).click()
  await expect(page.getByText("Config saved", { exact: true })).toBeVisible()
})
