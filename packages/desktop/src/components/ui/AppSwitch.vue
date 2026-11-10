<script setup lang="ts">
const model = defineModel<boolean>({ required: true });
defineProps<{ disabled?: boolean; label?: string }>();
</script>

<template>
  <button
    type="button"
    class="switch"
    :class="{ on: model }"
    :disabled="disabled"
    role="switch"
    :aria-checked="model"
    :aria-label="label"
    @click="model = !model"
  />
</template>
