<script setup lang="ts">
import { ref } from "vue"
import { useQuery, useQueryClient } from "@tanstack/vue-query"
import { Button } from "@/components/ui/button"
import ConfirmDialog from "@/core/components/ConfirmDialog.vue"
import QueryState from "@/core/components/QueryState.vue"
import SectionCard from "@/core/components/SectionCard.vue"
import { notifyError, notifySuccess } from "@/core/notify"
import { integrityApi } from "../api"
import { useIntegrityI18n } from "../i18n"
const { t } = useIntegrityI18n()
const client = useQueryClient()
const confirmation = ref(false)
const busy = ref(false)
const transaction = useQuery({
  queryKey: ["integrity-transaction.status"],
  queryFn: integrityApi.transactionStatus,
  refetchOnWindowFocus: true,
})
async function recover() {
  if (busy.value) return
  busy.value = true
  try {
    const result = await integrityApi.recoverTransaction()
    await Promise.all(
      ["integrity-transaction.status", "play-integrity.status", "play-integrity.config"].map(
        (key) => client.invalidateQueries({ queryKey: [key] }),
      ),
    )
    notifySuccess(t(result.restart_required ? "reboot" : "recovered"))
  } catch (error) {
    notifyError(error)
  } finally {
    busy.value = false
  }
}
</script>
<template>
  <QueryState
    :loading="transaction.isPending.value"
    :error="transaction.error.value"
    @retry="transaction.refetch()"
  >
    <SectionCard
      v-if="transaction.data.value?.pending"
      :title="t('recoveryTitle')"
      :description="t('recoveryHelp')"
    >
      <p class="mb-3 break-all font-mono text-xs">{{ transaction.data.value.journal_path }}</p>
      <Button :disabled="busy" @click="confirmation = true">{{ t("recover") }}</Button>
    </SectionCard>
  </QueryState>
  <ConfirmDialog
    v-model:open="confirmation"
    :title="t('recoveryTitle')"
    :description="t('recoveryHelp')"
    :confirm-label="t('recover')"
    @confirm="recover"
  />
</template>
