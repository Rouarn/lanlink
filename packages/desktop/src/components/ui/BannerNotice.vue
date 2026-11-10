<script setup lang="ts">
defineProps<{ title: string; text?: string | null }>();
</script>

<template>
  <div class="banner">
    <div class="banner-main">
      <span class="banner-title">{{ title }}</span>
      <span v-if="text" class="banner-text">{{ text }}</span>
    </div>
    <slot name="action" />
  </div>
</template>
