<script setup lang="ts">
import { useQuery, useQueryClient } from "@tanstack/vue-query"
import { Button } from "@/components/ui/button"
import QueryState from "@/core/components/QueryState.vue"
import SectionCard from "@/core/components/SectionCard.vue"
import { libraryApi } from "../library-api"
import { useIntegrityI18n } from "../i18n"
import SettingDetails from "./SettingDetails.vue"
const { t } = useIntegrityI18n()
const client = useQueryClient()
const data = useQuery({ queryKey: ["play-integrity.inspection"], queryFn: libraryApi.inspect })
async function refresh() {
  await Promise.all([
    data.refetch(),
    client.invalidateQueries({ queryKey: ["play-integrity.config"] }),
  ])
}
</script>
<template>
  <SectionCard :title="t('inspection.title')" :description="t('inspection.help')">
    <Button variant="secondary" :disabled="data.isFetching.value" @click="refresh()">{{
      t("inspection.refresh")
    }}</Button>
    <QueryState :loading="data.isPending.value" :error="data.error.value" @retry="data.refetch()">
      <p
        v-if="data.data.value?.read_error"
        role="status"
        class="text-muted-foreground mt-3 text-sm"
      >
        {{ t("inspection.unavailable") }}
      </p>
      <p class="text-muted-foreground mt-3 text-sm">{{ t("inspection.unobserved") }}</p>
      <SettingDetails :title="t('inspection.values')" class="mt-3">
        <dl class="flex flex-col gap-4">
          <div
            v-for="row in data.data.value?.rows"
            :key="row.field"
            class="rounded-xl border p-3 text-sm"
          >
            <dt dir="ltr" class="font-mono text-xs break-all">
              {{ row.field }} ↔ {{ row.property }}
            </dt>
            <dd class="mt-3 grid min-w-0 gap-3 sm:grid-cols-3">
              <div
                v-for="[label, value] in [
                  [t('inspection.system'), row.system],
                  ['Build', row.saved_build],
                  [t('editing.property'), row.saved_property],
                ]"
                :key="label ?? ''"
              >
                <p class="text-muted-foreground text-xs">{{ label }}</p>
                <p class="mt-1 font-mono break-all">{{ value || t("inspection.unknown") }}</p>
              </div>
            </dd>
          </div>
        </dl>
      </SettingDetails>
    </QueryState>
  </SectionCard>
</template>
