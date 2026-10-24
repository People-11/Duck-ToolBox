<script setup lang="ts">
import { Button } from "@/components/ui/button"
import { ref } from "vue"
import { ChevronDown } from "@lucide/vue"
import { Input } from "@/components/ui/input"
import { Field, FieldLabel } from "@/components/ui/field"
import { identityPairs, pairedDraft } from "../fields"
import { useIntegrityI18n } from "../i18n"
import SettingDetails from "./SettingDetails.vue"
const entries = defineModel<Record<string, string>>({ required: true })
const { t } = useIntegrityI18n()
const showProperties = ref(
  identityPairs.some(
    ([field, property]) =>
      entries.value[field] &&
      entries.value[property] &&
      entries.value[field] !== entries.value[property],
  ),
)
function sync() {
  entries.value = pairedDraft(entries.value)
  showProperties.value = true
}
const labels = {
  MODEL: "editing.MODEL",
  FINGERPRINT: "editing.FINGERPRINT",
  SECURITY_PATCH: "editing.SECURITY_PATCH",
  DEVICE_INITIAL_SDK_INT: "editing.DEVICE_INITIAL_SDK_INT",
} as const
function label(field: string) {
  const key = labels[field as keyof typeof labels]
  return key ? t(key) : field
}
function set(key: string, value: string) {
  entries.value = { ...entries.value, [key]: value }
}
</script>
<template>
  <div class="flex flex-col gap-5">
    <div>
      <div class="flex flex-wrap gap-2">
        <Button variant="secondary" @click="sync()">{{ t("editing.sync") }}</Button>
        <Button
          variant="ghost"
          :aria-expanded="showProperties"
          @click="showProperties = !showProperties"
          >{{ t("editing.properties") }}<ChevronDown :class="showProperties ? 'rotate-180' : ''"
        /></Button>
      </div>
      <p class="text-muted-foreground mt-2 text-sm">{{ t("editing.syncHelp") }}</p>
    </div>
    <template
      v-for="(pairs, index) in [identityPairs.slice(0, 4), identityPairs.slice(4)]"
      :key="index"
    >
      <component
        :is="index ? SettingDetails : 'div'"
        :title="index ? t('editing.more') : undefined"
        class="flex flex-col gap-5"
      >
        <div class="flex flex-col gap-5">
          <div
            v-for="[field, property] in pairs"
            :key="field"
            class="grid min-w-0 gap-3 rounded-xl border p-3"
            :class="showProperties ? 'sm:grid-cols-2' : ''"
          >
            <Field>
              <FieldLabel :for="`pif-${field}`">{{ label(field) }}</FieldLabel>
              <span dir="ltr" class="text-muted-foreground font-mono text-xs break-all">{{
                field
              }}</span>
              <Input
                :id="`pif-${field}`"
                :aria-label="label(field) === field ? field : `${label(field)} · ${field}`"
                :model-value="entries[field] ?? ''"
                class="font-mono text-sm"
                autocapitalize="off"
                spellcheck="false"
                @update:model-value="set(field, String($event))"
              />
            </Field>
            <Field v-if="showProperties">
              <FieldLabel :for="`pif-${property}`">{{ t("editing.property") }}</FieldLabel>
              <span dir="ltr" class="text-muted-foreground font-mono text-xs break-all">{{
                property
              }}</span>
              <Input
                :id="`pif-${property}`"
                :aria-label="`${t('editing.property')} · ${property}`"
                :model-value="entries[property] ?? ''"
                class="font-mono text-sm"
                autocapitalize="off"
                spellcheck="false"
                @update:model-value="set(property, String($event))"
              />
            </Field>
          </div>
        </div>
      </component>
    </template>
  </div>
</template>
