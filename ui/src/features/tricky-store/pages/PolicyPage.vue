<script setup lang="ts">
import { useMutation, useQueryClient } from "@tanstack/vue-query"
import { Save } from "@lucide/vue"
import { ref, watch } from "vue"

import { Button } from "@/components/ui/button"
import { Spinner } from "@/components/ui/spinner"
import QueryState from "@/core/components/QueryState.vue"
import SectionCard from "@/core/components/SectionCard.vue"
import { notifySuccess } from "@/core/notify"

import type { Policy } from "../api"
import PolicyEditor from "../components/PolicyEditor.vue"
import { useTrickyI18n } from "../i18n"
import { STATUS_KEY, useStatusQuery } from "../queries"
import { useTrickyStore } from "../store"
import { samePolicy } from "../selection"

const { t } = useTrickyI18n()
const store = useTrickyStore()
const status = useStatusQuery()
const client = useQueryClient()
const policy = ref<Policy>({})
let baseline: Policy | undefined

watch(
  status.data,
  (next) => {
    if (!next) return
    if (baseline && !samePolicy(policy.value, baseline)) return
    store.load(next)
    policy.value = { ...next.config.default_policy }
    baseline = { ...next.config.default_policy }
  },
  { immediate: true },
)

const save = useMutation({
  mutationFn: async () => {
    await client.cancelQueries({ queryKey: STATUS_KEY })
    const saved = await store.saveOnly({ defaultPolicy: policy.value })
    client.setQueryData(STATUS_KEY, store.status)
    return saved
  },
  onSuccess: (saved) => {
    policy.value = { ...(saved.config?.default_policy ?? policy.value) }
    baseline = { ...policy.value }
    notifySuccess(t("policy.saved"), saved.restart_required ? t("policy.restart") : undefined)
  },
})
</script>

<template>
  <div class="flex flex-col gap-6 p-4">
    <fieldset :disabled="save.isPending.value || store.saving" class="flex min-w-0 flex-col gap-6">
      <QueryState
        :loading="status.isPending.value"
        :error="status.error.value"
        @retry="status.refetch()"
      >
        <p v-if="!store.schema?.default_policy.length" class="text-muted-foreground px-4 text-sm">
          {{ t("policy.none") }}
        </p>
        <template v-else>
          <SectionCard :description="t('policy.hint')">
            <PolicyEditor
              v-model="policy"
              :fields="store.schema.default_policy"
              id-prefix="default-policy"
            />
          </SectionCard>
          <Button size="lg" :disabled="save.isPending.value" @click="save.mutate()">
            <Spinner v-if="save.isPending.value" />
            <Save v-else />
            {{ t("ta.functional_button_save") }}
          </Button>
        </template>
      </QueryState>
    </fieldset>
  </div>
</template>
