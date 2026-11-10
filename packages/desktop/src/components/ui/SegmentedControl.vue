<script setup lang="ts" generic="T extends string">
import type { SegmentedItem } from "../../types";

defineProps<{ items: readonly SegmentedItem<T>[] }>();

const model = defineModel<T>({ required: true });
</script>

<template>
  <div class="segmented">
    <button
      v-for="item in items"
      :key="item.key"
      class="segment"
      :class="{ active: model === item.key }"
      @click="model = item.key"
    >
      {{ item.label }}
    </button>
  </div>
</template>
