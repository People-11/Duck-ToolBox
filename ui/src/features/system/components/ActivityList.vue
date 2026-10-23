<script setup lang="ts">
import { useInfiniteQuery } from "@tanstack/vue-query"
import { computed } from "vue"
import { CircleCheck, CircleX } from "@lucide/vue"

import { Button } from "@/components/ui/button"
import { Item, ItemContent, ItemDescription, ItemMedia, ItemTitle } from "@/components/ui/item"
import QueryState from "@/core/components/QueryState.vue"
import { formatRelative } from "@/core/format"

import { systemApi } from "../api"
import { useSystemI18n } from "../i18n"

const props = withDefaults(defineProps<{ limit?: number; paginated?: boolean }>(), {
  limit: 8,
  paginated: false,
})
const { t } = useSystemI18n()
const log = useInfiniteQuery({
  queryKey: ["system.log", () => props.limit, () => props.paginated],
  initialPageParam: 0,
  queryFn: ({ pageParam }) => systemApi.log(props.limit, pageParam),
  getNextPageParam: (lastPage) => lastPage.next_offset ?? undefined,
  staleTime: 0,
})
const entries = computed(() => log.data.value?.pages.flatMap((page) => page.entries) ?? [])
</script>

<template>
  <QueryState
    :loading="log.isPending.value"
    :error="log.data.value ? null : log.error.value"
    :skeleton-rows="3"
    @retry="log.refetch()"
  >
    <p v-if="!entries.length" class="text-muted-foreground text-sm">
      {{ t("activity.empty") }}
    </p>
    <div v-else class="-mx-2 flex flex-col">
      <Item v-for="(entry, index) in entries" :key="`${entry.ts}-${index}`" size="sm">
        <ItemMedia variant="icon" :class="entry.ok ? '' : 'bg-destructive/10 text-destructive'">
          <CircleCheck v-if="entry.ok" />
          <CircleX v-else />
        </ItemMedia>
        <ItemContent class="min-w-0">
          <ItemTitle class="w-full font-mono text-sm leading-6 [overflow-wrap:anywhere]">{{
            entry.command
          }}</ItemTitle>
          <ItemDescription class="line-clamp-2">
            {{ formatRelative(entry.ts) }}
            <template v-if="!entry.ok">
              · {{ entry.code ?? t("activity.failed") }}: {{ entry.message }}</template
            >
          </ItemDescription>
        </ItemContent>
      </Item>
    </div>
    <div
      v-if="paginated && log.isFetchNextPageError.value"
      class="flex flex-col items-center gap-2 py-3"
    >
      <p class="text-destructive text-sm">{{ String(log.error.value) }}</p>
      <Button variant="outline" @click="log.fetchNextPage()">
        {{ t("activity.retryMore") }}
      </Button>
    </div>
    <Button
      v-else-if="paginated && log.hasNextPage.value"
      variant="outline"
      class="mt-3 w-full"
      :disabled="log.isFetchingNextPage.value"
      @click="log.fetchNextPage()"
    >
      {{ log.isFetchingNextPage.value ? t("activity.loadingMore") : t("activity.loadMore") }}
    </Button>
  </QueryState>
</template>
