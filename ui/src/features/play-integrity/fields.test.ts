import { describe, expect, it } from "vitest"
import { pairedDraft } from "./fields"

describe("explicit field synchronization", () => {
  it("keeps unrelated and empty-source properties and leaves the original draft untouched", () => {
    const draft = {
      MODEL: "Chosen",
      "ro.product.model": "Different",
      SECURITY_PATCH: "",
      "ro.build.version.security_patch": "2026-09-05",
      "custom.flag": "retain",
      spoofProvider: "1",
    }
    const next = pairedDraft(draft)
    expect(next["ro.product.model"]).toBe("Chosen")
    expect(next["ro.build.version.security_patch"]).toBe("2026-09-05")
    expect(next["custom.flag"]).toBe("retain")
    expect(next.spoofProvider).toBe("1")
    expect(draft["ro.product.model"]).toBe("Different")
  })
})
