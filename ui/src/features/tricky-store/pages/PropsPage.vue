<script setup lang="ts">
import { useMutation, useQueryClient } from "@tanstack/vue-query"
import { RefreshCw, Save } from "@lucide/vue"
import { computed, ref, watch } from "vue"

import { Button } from "@/components/ui/button"
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldError,
  FieldLabel,
} from "@/components/ui/field"
import { Spinner } from "@/components/ui/spinner"
import { Switch } from "@/components/ui/switch"
import { Textarea } from "@/components/ui/textarea"
import QueryState from "@/core/components/QueryState.vue"
import SectionCard from "@/core/components/SectionCard.vue"
import { notifySuccess } from "@/core/notify"

import { trickyStoreApi, type Status } from "../api"
import { useTrickyI18n } from "../i18n"
import { STATUS_KEY, useStatusQuery } from "../queries"

const { t } = useTrickyI18n()
const client = useQueryClient()
const status = useStatusQuery()
const handler = ref(true)
const bootHash = ref("")
const expected = ref<Status["props"]>()
const hash = computed(() => bootHash.value.replace(/\s/g, "").toLowerCase())

function load(next: Status) {
  handler.value = next.props.prop_handler_enabled
  bootHash.value = next.props.boot_hash ?? ""
  expected.value = { ...next.props }
}

watch(
  status.data,
  (next) => {
    if (!next) return
    if (
      expected.value &&
      (handler.value !== expected.value.prop_handler_enabled ||
        hash.value !== (expected.value.boot_hash ?? ""))
    )
      return
    load(next)
  },
  { immediate: true },
)

async function refresh() {
  const result = await status.refetch()
  if (result.isSuccess && result.data) load(result.data)
}
const invalid = computed(
  () => hash.value !== "" && !/^(?:[0-9a-f]{64}|[0-9a-f]{128})$/.test(hash.value),
)

const save = useMutation({
  mutationFn: () =>
    trickyStoreApi.saveProps({
      prop_handler_enabled: handler.value,
      boot_hash: hash.value || null,
      expected: expected.value,
    }),
  meta: { errorTitle: () => t("ta.prompt_boot_hash_set_error") },
  onSuccess: (result) => {
    handler.value = result.prop_handler_enabled
    bootHash.value = result.boot_hash ?? ""
    expected.value = {
      prop_handler_enabled: result.prop_handler_enabled,
      boot_hash: result.boot_hash,
    }
    const hint = result.synced_backend_policy ? `${t("props.synced")} ` : ""
    notifySuccess(
      result.boot_hash ? t("ta.prompt_boot_hash_set") : t("ta.prompt_saved_target"),
      result.restart_required ? `${hint}${t("policy.restart")}` : undefined,
    )
    void client.invalidateQueries({ queryKey: STATUS_KEY })
  },
})
</script>

<template>
  <div class="flex flex-col gap-6 p-4">
    <fieldset
      :disabled="save.isPending.value || status.isFetching.value"
      class="flex min-w-0 flex-col gap-6"
    >
      <QueryState
        :loading="status.isPending.value"
        :error="status.error.value"
        @retry="status.refetch()"
      >
        <SectionCard>
          <div class="flex flex-col gap-6">
            <Field orientation="horizontal">
              <FieldContent>
                <FieldLabel for="prop-handler">{{ t("ta.prop_handler") }}</FieldLabel>
                <FieldDescription>{{ t("props.handlerHint") }}</FieldDescription>
              </FieldContent>
              <Switch id="prop-handler" v-model="handler" />
            </Field>
            <Field :data-invalid="invalid || undefined">
              <FieldLabel for="boot-hash">{{ t("ta.boot_hash_title") }}</FieldLabel>
              <Textarea
                id="boot-hash"
                v-model="bootHash"
                rows="3"
                class="font-mono text-sm"
                :placeholder="t('props.invalidHash')"
                autocapitalize="off"
                spellcheck="false"
                :aria-invalid="invalid"
              />
              <FieldError v-if="invalid">{{ t("props.invalidHash") }}</FieldError>
              <FieldDescription v-else>{{ t("props.bootHashHint") }}</FieldDescription>
            </Field>
          </div>
        </SectionCard>
        <Button variant="outline" @click="refresh"> <RefreshCw />{{ t("list.refresh") }} </Button>
        <Button size="lg" :disabled="invalid || save.isPending.value" @click="save.mutate()">
          <Spinner v-if="save.isPending.value" />
          <Save v-else />
          {{ t("ta.functional_button_save") }}
        </Button>
      </QueryState>
    </fieldset>
  </div>
</template>
