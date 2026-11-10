<script setup lang="ts">
defineProps<{ title: string }>();
</script>

<template>
  <section class="group">
    <div class="group-title" :class="{ 'has-action': !!$slots.action }">
      {{ title }}
      <slot name="action" />
    </div>
    <div class="group-body">
      <slot />
    </div>
  </section>
</template>
