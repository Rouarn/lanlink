<script setup lang="ts">
import type { UiState } from "../../types";

defineProps<{ state: UiState }>();
</script>

<template>
  <span class="status-dot" :class="state"></span>
</template>
