<script setup lang="ts">
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import SectionCard from "@/core/components/SectionCard.vue"
import type { SecurityPatchPreview } from "@/core/features"
import type { Preview } from "../api"
import { useIntegrityI18n } from "../i18n"
defineProps<{
  preview: Preview
  patches: { target: { id: string }; preview: SecurityPatchPreview; selected: boolean }[]
  busy: boolean
}>()
const emit = defineEmits<{ apply: []; close: []; select: [id: string, selected: boolean] }>()
const { t } = useIntegrityI18n()
</script>
<template>
  <SectionCard :title="t('preview')" :description="t('previewHelp')">
    <p v-if="preview.metadata" class="mb-3 text-sm">
      {{ t("dates") }}: {{ preview.metadata.released ?? "—" }} →
      {{ preview.metadata.estimated_expiry ?? "—" }}
    </p>
    <div
      v-if="preview.warnings.length"
      class="bg-muted mb-4 flex flex-col gap-2 rounded-lg p-4"
      role="status"
    >
      <p
        v-for="(warning, index) in preview.warnings"
        :key="index"
        class="text-muted-foreground text-sm break-words"
      >
        {{ t(`warnings.${warning.code}`)
        }}<span v-if="warning.fields.length" class="font-mono">
          · {{ warning.fields.join(", ") }}</span
        >
      </p>
    </div>
    <div class="overflow-x-auto rounded-xl border border-border/70">
      <table class="w-full text-start text-xs">
        <thead class="bg-muted/60">
          <tr>
            <th class="p-2 text-start">{{ t("field") }}</th>
            <th class="p-2 text-start">{{ t("before") }}</th>
            <th class="p-2 text-start">{{ t("after") }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="change in preview.changes" :key="change.key" class="border-t">
            <td class="p-2 font-mono">{{ change.key }}</td>
            <td class="max-w-48 p-2 break-all">{{ change.before ?? "—" }}</td>
            <td class="max-w-48 p-2 break-all">{{ change.after ?? "—" }}</td>
          </tr>
        </tbody>
      </table>
    </div>
    <div
      v-for="patch in patches"
      :key="patch.target.id"
      class="mt-4 rounded-xl border border-border/70 bg-muted/20 p-4"
    >
      <label class="flex min-h-11 items-center gap-3"
        ><Checkbox
          :model-value="patch.selected"
          @update:model-value="emit('select', patch.target.id, $event === true)"
        />{{ t("syncPatch") }} ({{ patch.preview.backend }})</label
      >
      <p
        v-for="change in patch.preview.changes"
        :key="change.key"
        class="text-muted-foreground break-all font-mono text-xs"
      >
        {{ change.key }}: {{ change.before ?? "—" }} → {{ change.after }}
      </p>
    </div>
    <div class="mt-4 flex flex-wrap gap-2">
      <Button :disabled="busy" @click="emit('apply')">{{ t("apply") }}</Button
      ><Button variant="outline" :disabled="busy" @click="emit('close')">{{ t("dismiss") }}</Button>
    </div>
  </SectionCard>
</template>
