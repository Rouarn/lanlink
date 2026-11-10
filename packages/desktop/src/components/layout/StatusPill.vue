<script setup lang="ts">
import { computed } from "vue";
import { useApp } from "../../stores/app";
import { useConnection } from "../../stores/connection";

const { go, goRoom } = useApp();
const { pill, phase } = useConnection();

const toneClass = computed(() => `pill-${pill.value.tone}`);

function jump(): void {
  if (phase.value === "unconfigured") {
    go("settings");
    return;
  }
  goRoom();
}
</script>

<template>
  <button type="button" class="status-pill" :class="toneClass" @click="jump">
    <span class="dot" :class="pill.tone" />
    {{ pill.label }}
  </button>
</template>

<style scoped>
.status-pill {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  height: 28px;
  padding: 0 10px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: var(--surface-2);
  font-size: var(--text-sm);
  font-weight: 600;
  color: var(--text-2);
  transition: background var(--dur-base) var(--ease), color var(--dur-base) var(--ease);
}

.status-pill:hover {
  background: var(--surface);
}

.status-pill.pill-ok {
  border-color: transparent;
  background: var(--ok-soft);
  color: var(--ok);
}

.status-pill.pill-warn {
  border-color: transparent;
  background: var(--warn-soft);
  color: var(--warn);
}

.status-pill.pill-danger {
  border-color: transparent;
  background: var(--danger-soft);
  color: var(--danger);
}

.status-pill.pill-warn .dot {
  animation: pulse 1.5s var(--ease) infinite;
}

@keyframes pulse {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.45;
  }
}
</style>
