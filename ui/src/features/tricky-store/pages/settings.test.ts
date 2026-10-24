import { VueQueryPlugin, QueryClient } from "@tanstack/vue-query"
import { flushPromises, mount } from "@vue/test-utils"
import { createPinia } from "pinia"
import { beforeEach, describe, expect, it, vi } from "vitest"
import { ref, shallowRef } from "vue"

import type { Status } from "../api"
import { trickyStoreApi } from "../api"
import PolicyPage from "./PolicyPage.vue"
import PropsPage from "./PropsPage.vue"

vi.mock("../i18n", () => ({ useTrickyI18n: () => ({ t: (key: string) => key }) }))
vi.mock("@/core/notify", () => ({ notifySuccess: vi.fn() }))
vi.mock("../queries", () => ({
  STATUS_KEY: ["tricky-store", "status"],
  useStatusQuery: () => query,
}))

const initial: Status = {
  backends: [],
  active: null,
  config_error: null,
  keybox: null,
  config: { targets: [], default_policy: { os_patch: "prop" }, per_app_policy: {} },
  packages: [],
  system_apps: [],
  auto_add_new_apps: false,
  root_manager: null,
  props: { prop_handler_enabled: true, boot_hash: null },
  schema: {
    supports_app_mode: true,
    supports_per_app_policy: true,
    default_policy: [
      { key: "os_patch", label: "Patch", kind: "text", options: [], multiline: false },
    ],
  },
}
let query: {
  data: ReturnType<typeof shallowRef<Status>>
  isPending: ReturnType<typeof ref<boolean>>
  isFetching: ReturnType<typeof ref<boolean>>
  error: ReturnType<typeof ref<null>>
  refetch: () => Promise<{ isSuccess: boolean; data: Status }>
}
const global = () => ({
  plugins: [
    createPinia(),
    [VueQueryPlugin, { queryClient: new QueryClient() }] as [
      typeof VueQueryPlugin,
      { queryClient: QueryClient },
    ],
  ],
  stubs: {
    QueryState: { template: "<slot />" },
    SectionCard: { template: "<slot />" },
    PolicyEditor: {
      props: ["modelValue"],
      emits: ["update:modelValue"],
      template: `<input :value="modelValue.os_patch" @input="$emit('update:modelValue', { os_patch: $event.target.value })" />`,
    },
  },
})

beforeEach(() => {
  query = {
    data: shallowRef(structuredClone(initial)),
    isPending: ref(false),
    isFetching: ref(false),
    error: ref(null),
    refetch: async () => ({ isSuccess: true, data: query.data.value! }),
  }
  vi.restoreAllMocks()
})

describe("settings refresh", () => {
  it("preserves the original prop baseline across repeated refreshes and reloads explicitly", async () => {
    const save = vi.spyOn(trickyStoreApi, "saveProps").mockImplementation(async (request) => ({
      ...request,
      applied_boot_hash: false,
      synced_backend_policy: false,
      restart_required: false,
    }))
    const page = mount(PropsPage, { global: global() })
    const input = page.get("textarea")
    const draft = "ab".repeat(32)
    await input.setValue(draft)
    query.data.value = { ...initial, props: { prop_handler_enabled: true, boot_hash: draft } }
    await flushPromises()
    const external = "cd".repeat(32)
    query.data.value = { ...initial, props: { prop_handler_enabled: true, boot_hash: external } }
    await flushPromises()
    expect((input.element as HTMLTextAreaElement).value).toBe(draft)
    await page
      .findAll("button")
      .find((button) => button.text().includes("list.refresh"))!
      .trigger("click")
    await flushPromises()
    expect((input.element as HTMLTextAreaElement).value).toBe(external)
    await page
      .findAll("button")
      .find((button) => button.text().includes("ta.functional_button_save"))!
      .trigger("click")
    await flushPromises()
    expect(save).toHaveBeenCalledWith({
      prop_handler_enabled: true,
      boot_hash: external,
      expected: { prop_handler_enabled: true, boot_hash: external },
    })
    page.unmount()
  })

  it("keeps unsaved policy edits when a background query observes another policy", async () => {
    const page = mount(PolicyPage, { global: global() })
    await page.get("input").setValue("202610")
    query.data.value = {
      ...initial,
      config: { ...initial.config, default_policy: { os_patch: "202609" } },
    }
    await flushPromises()
    expect((page.get("input").element as HTMLInputElement).value).toBe("202610")
    page.unmount()
  })
})
