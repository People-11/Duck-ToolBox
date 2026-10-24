<script setup lang="ts">
import { ChevronDown } from "@lucide/vue"
import { Button } from "@/components/ui/button"
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible"
defineProps<{ title: string }>()
</script>
<template>
  <Collapsible class="min-w-0">
    <CollapsibleTrigger as-child>
      <Button variant="ghost" class="group w-full justify-between whitespace-normal text-start">
        <span>{{ title }}</span
        ><ChevronDown
          class="shrink-0 transition-transform group-data-[state=open]:rotate-180 motion-reduce:transition-none"
        />
      </Button>
    </CollapsibleTrigger>
    <CollapsibleContent class="pt-3"><slot /></CollapsibleContent>
  </Collapsible>
</template>
