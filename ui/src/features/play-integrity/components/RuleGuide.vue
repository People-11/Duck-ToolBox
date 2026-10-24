<script setup lang="ts">
import { Button } from "@/components/ui/button"
import SectionCard from "@/core/components/SectionCard.vue"
import { openExternal } from "@/core/links"
import { useIntegrityI18n } from "../i18n"
import SettingDetails from "./SettingDetails.vue"
const { t } = useIntegrityI18n()
</script>
<template>
  <SectionCard :title="t('guide.title')" :description="`${t('guide.checked')} · 2026-10-24`">
    <p class="text-sm">{{ t("guide.pif") }}</p>
    <SettingDetails :title="t('guide.officialTitle')" class="mt-3">
      <p class="text-muted-foreground text-sm">{{ t("guide.official") }}</p>
      <Button
        variant="link"
        class="mt-2"
        @click="
          openExternal(
            'https://developer.android.com/google/play/integrity/verdicts#device-integrity-field',
          )
        "
        >Google · Play Integrity</Button
      >
    </SettingDetails>
    <SettingDetails :title="t('guide.communityTitle')" class="mt-2">
      <p class="text-muted-foreground text-sm">{{ t("guide.community") }}</p>
      <Button
        variant="link"
        class="mt-2"
        @click="openExternal('https://blog.pixelos.net/blogs/2026-10-19-October-Update/')"
        >PixelOS · 2026-10</Button
      >
    </SettingDetails>
  </SectionCard>
</template>
