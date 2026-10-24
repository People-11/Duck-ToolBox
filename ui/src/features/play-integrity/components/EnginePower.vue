<script setup lang="ts">
import { ref } from "vue"
import { useQuery, useQueryClient } from "@tanstack/vue-query"
import { Power } from "@lucide/vue"
import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { Field, FieldContent, FieldDescription, FieldLabel } from "@/components/ui/field"
import SectionCard from "@/core/components/SectionCard.vue"
import { notifyError, notifySuccess } from "@/core/notify"
import { integrityApi } from "../api"
import { useIntegrityI18n } from "../i18n"

defineProps<{ stopOnly?: boolean }>()
const { t } = useIntegrityI18n()
const client = useQueryClient()
const power = useQuery({
  queryKey: ["play-integrity.power"],
  queryFn: integrityApi.power,
  refetchOnWindowFocus: true,
})
const enabling = ref(false),
  stopping = ref(false)
async function refresh() {
  await Promise.all(
    ["play-integrity.power", "play-integrity.status"].map((key) =>
      client.invalidateQueries({ queryKey: [key] }),
    ),
  )
}
async function stop() {
  if (stopping.value) return
  stopping.value = true
  try {
    await integrityApi.disable()
    await refresh()
    notifySuccess(t("disabledHelp"))
  } catch (error) {
    notifyError(error)
    await refresh()
  } finally {
    stopping.value = false
  }
}
async function toggle(enabled: boolean) {
  if (!enabled) {
    await stop()
    return
  }
  if (enabling.value || stopping.value || !power.data.value) return
  enabling.value = true
  try {
    await integrityApi.options({
      enabled: true,
      expected_stop_revision: power.data.value.stop_revision,
    })
    await refresh()
    notifySuccess(t("reboot"))
  } catch (error) {
    notifyError(error)
  } finally {
    enabling.value = false
  }
}
</script>
<template>
  <!-- Keep the independent emergency stop available even when the config/status queries fail. -->
  <section v-if="stopOnly" class="flex flex-col items-start">
    <Button variant="outline" :disabled="stopping" @click="stop">
      <Power />{{ t(power.data.value?.cleanup_pending ? "retryCleanup" : "disableNow") }}
    </Button>
    <p v-if="power.data.value?.enabled === false" class="text-muted-foreground mt-2 text-xs">
      {{ t("disabledHelp") }}
    </p>
    <p v-if="power.data.value?.cleanup_pending" role="alert" class="text-destructive mt-2 text-sm">
      {{ t("cleanupPending") }}
    </p>
    <p v-if="power.error.value" role="alert" class="text-destructive mt-2 text-sm">
      {{ String(power.error.value) }}
    </p>
    <p
      v-for="issue in power.data.value?.issues ?? []"
      :key="issue.step"
      class="mt-2 text-xs break-words"
    >
      {{ issue.step }}: {{ issue.message }}
    </p>
  </section>
  <section v-else>
    <SectionCard :title="t('enabled')" :description="t('powerHelp')">
      <Field orientation="horizontal">
        <FieldContent
          ><FieldLabel for="pif-enabled">{{ t("enabled") }}</FieldLabel
          ><FieldDescription>{{
            t(power.data.value?.enabled ? "enableHelp" : "disabledHelp")
          }}</FieldDescription></FieldContent
        >
        <Switch
          id="pif-enabled"
          :model-value="power.data.value?.enabled ?? false"
          :disabled="power.isPending.value || enabling || stopping || !!power.error.value"
          @update:model-value="toggle"
        />
      </Field>
      <Button class="mt-3" variant="outline" :disabled="stopping" @click="stop"
        ><Power />{{ t(power.data.value?.cleanup_pending ? "retryCleanup" : "disableNow") }}</Button
      >
      <p class="text-muted-foreground mt-3 text-xs">{{ t("denylistRestoreHelp") }}</p>
      <p class="text-muted-foreground mt-2 text-xs">{{ t("persistRestoreHelp") }}</p>
      <p v-if="power.error.value" role="alert" class="text-destructive mt-3 text-sm">
        {{ String(power.error.value) }}
      </p>
      <p
        v-if="power.data.value?.cleanup_pending"
        role="alert"
        class="text-destructive mt-3 text-sm"
      >
        {{ t("cleanupPending") }}
      </p>
      <p
        v-for="issue in power.data.value?.issues ?? []"
        :key="issue.step"
        class="mt-2 text-xs break-words"
      >
        {{ issue.step }}: {{ issue.message }}
      </p>
    </SectionCard>
  </section>
</template>
