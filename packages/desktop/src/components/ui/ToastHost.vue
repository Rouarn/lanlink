<script setup lang="ts">
import { useApp } from "../../stores/app";

const { toastText } = useApp();
</script>

<template>
  <Transition name="toast">
    <div v-if="toastText" class="toast" role="status">{{ toastText }}</div>
  </Transition>
</template>

<style scoped>
.toast {
  position: fixed;
  left: 50%;
  bottom: 22px;
  z-index: 60;
  transform: translateX(-50%);
  padding: 8px 14px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: var(--surface);
  box-shadow: 0 12px 30px -12px rgba(var(--shadow-rgb), 0.45);
  font-size: var(--text-sm);
  font-weight: 600;
  color: var(--text);
  white-space: nowrap;
}

.toast-enter-active,
.toast-leave-active {
  transition: opacity var(--dur-base) var(--ease), transform var(--dur-base) var(--ease);
}

.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translate(-50%, 6px);
}
</style>
