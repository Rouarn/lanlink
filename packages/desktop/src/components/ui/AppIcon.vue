<script setup lang="ts">
withDefaults(defineProps<{ name: string; size?: number }>(), { size: 16 });
</script>

<template>
  <svg class="icon" :width="size" :height="size" aria-hidden="true" focusable="false">
    <use :href="`#i-${name}`" />
  </svg>
</template>
