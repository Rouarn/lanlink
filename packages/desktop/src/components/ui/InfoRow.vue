<script setup lang="ts">
withDefaults(defineProps<{ label?: string; column?: boolean }>(), {
  label: "",
  column: false,
});
</script>

<template>
  <div class="row" :class="{ column }">
    <span class="row-label">{{ label }}</span>
    <slot />
  </div>
</template>
