import { expect, test } from "./fixtures"

test("loads a large command history in bounded pages without losing records", async ({
  page,
  open,
}) => {
  await open("/system")
  await page.evaluate(() => {
    const scope = window as unknown as Record<string, unknown>
    const host = scope.ksu as {
      exec: (command: string, options: string, callback: string) => void
    }
    const original = host.exec
    scope.__duckLogOffsets = [] as number[]
    host.exec = (command, options, callback) => {
      if (!command.includes("'system' 'log'")) {
        original(command, options, callback)
        return
      }
      const offset = Number(command.match(/'--offset' '(\d+)'/)?.[1] ?? 0)
      const limit = Number(command.match(/'--limit' '(\d+)'/)?.[1] ?? 20)
      if (limit === 20) (scope.__duckLogOffsets as number[]).push(offset)
      const entries = Array.from({ length: 45 }, (_, index) => ({
        ts: 45 - index,
        command: `history.${44 - index}`,
        ok: true,
      }))
      const data = {
        entries: entries.slice(offset, offset + limit),
        next_offset: offset + limit < entries.length ? offset + limit : null,
      }
      const envelope = JSON.stringify({
        ok: true,
        api: 1,
        command: "system.log",
        data,
        error: null,
      })
      setTimeout(() => {
        ;(scope[callback] as (...args: unknown[]) => void)?.(0, envelope, "")
      }, 0)
    }
  })
  await page.evaluate(() => {
    location.hash = "#/system/activity"
  })

  const more = page.getByRole("button", { name: "Load more" })
  await expect(page.getByText("history.44")).toBeVisible()
  await expect(page.getByText("history.25")).toBeVisible()
  await expect(page.getByText("history.24")).toHaveCount(0)
  await more.click()
  await expect(page.getByText("history.24")).toBeVisible()
  await expect(page.getByText("history.5")).toBeVisible()
  await more.click()
  await expect(page.getByText("history.0")).toBeVisible()
  await expect(more).toHaveCount(0)
  const offsets = await page.evaluate(
    () => (window as unknown as { __duckLogOffsets: number[] }).__duckLogOffsets,
  )
  expect(offsets).toEqual([0, 20, 40])
})
