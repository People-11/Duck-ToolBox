import { defineStore } from "pinia"
import { computed, ref, shallowRef } from "vue"

import { DuckError } from "@/core/duckd"

import type { Policy, SaveRequest, Status, TargetMode } from "./api"
import { trickyStoreApi } from "./api"
import type { Draft } from "./selection"
import {
  deselect,
  draftFromStatus,
  sameDraft,
  samePolicy,
  select,
  toSaveRequest,
} from "./selection"

/**
 * Unsaved edits of the target list. Like Tricky Addon, taps only change this draft; the
 * Save button writes it to the active keystore module.
 */
export const useTrickyStore = defineStore("tricky-store", () => {
  const status = shallowRef<Status | null>(null)
  const draft = ref<Draft | null>(null)
  const saving = ref(false)
  let expectedConfig: Status["config"] | undefined
  let expectedBackend: SaveRequest["expected_backend"]

  const baseline = computed(() => (status.value ? draftFromStatus(status.value) : null))
  const dirty = computed(
    () => !!draft.value && !!baseline.value && !sameDraft(draft.value, baseline.value),
  )
  const schema = computed(() => status.value?.schema ?? null)

  /** Takes a fresh status; pending edits survive unless `discard` is set. */
  function load(next: Status, discard = false) {
    if (!discard && status.value === next && draft.value) return
    const keep = !discard && dirty.value
    status.value = next
    if (!keep || !draft.value) {
      draft.value = draftFromStatus(next)
      expectedConfig = structuredClone(next.config)
      expectedBackend = next.active?.backend
    }
  }

  function toggle(packageName: string) {
    const targets = draft.value!.targets
    if (targets.has(packageName)) targets.delete(packageName)
    else targets.set(packageName, "auto")
  }

  function setMode(packageName: string, mode: TargetMode, policy: Policy | null) {
    draft.value!.targets.set(packageName, mode)
    if (policy) draft.value!.perApp.set(packageName, policy)
    else draft.value!.perApp.delete(packageName)
  }

  const selectMany = (packages: Iterable<string>) => select(draft.value!, packages)
  const deselectMany = (packages: Iterable<string>) => deselect(draft.value!, packages)

  /** Checked system apps become targets and unchecked ones stop being targets, as upstream. */
  function setSystemApps(checked: ReadonlySet<string>) {
    const current = draft.value!
    for (const entry of status.value?.packages ?? []) {
      if (!entry.system) continue
      if (checked.has(entry.package_name)) select(current, [entry.package_name])
      else deselect(current, [entry.package_name])
    }
    current.systemApps = new Set(checked)
  }

  async function write(request: SaveRequest) {
    if (saving.value)
      throw new DuckError("operation_pending", "A save is already running", "tricky-store.save")
    saving.value = true
    try {
      const result = await trickyStoreApi.save({
        ...request,
        expected_config: expectedConfig,
        expected_backend: expectedBackend,
      })
      expectedConfig = result.config ?? {
        targets: request.targets,
        default_policy: request.default_policy,
        per_app_policy: request.per_app_policy,
      }
      if (status.value) {
        status.value = {
          ...status.value,
          config: expectedConfig,
          system_apps: request.system_apps,
          auto_add_new_apps: request.auto_add_new_apps,
        }
      }
      return result
    } finally {
      saving.value = false
    }
  }

  const save = () => write(toSaveRequest(draft.value!))

  /** Saves one setting on top of the saved config, leaving other pending edits pending. */
  async function saveOnly(patch: Partial<Pick<Draft, "defaultPolicy" | "autoAdd">>) {
    const saved = draftFromStatus(status.value!)
    const before = toSaveRequest(draft.value ?? saved)
    const request = toSaveRequest({ ...saved, ...patch })
    const result = await write(request)
    if (draft.value) {
      if (patch.defaultPolicy && samePolicy(draft.value.defaultPolicy, before.default_policy)) {
        draft.value.defaultPolicy = { ...request.default_policy }
      }
      if (patch.autoAdd !== undefined && draft.value.autoAdd === before.auto_add_new_apps) {
        draft.value.autoAdd = request.auto_add_new_apps
      }
    }
    return result
  }

  return {
    status,
    draft,
    dirty,
    saving,
    schema,
    load,
    toggle,
    setMode,
    selectMany,
    deselectMany,
    setSystemApps,
    save,
    saveOnly,
  }
})
