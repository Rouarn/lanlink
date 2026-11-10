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
  height: 24px;
  padding: 0 10px;
  border-radius: 999px;
  background: var(--surface-2);
  font-size: var(--text-xs);
  font-weight: 650;
  color: var(--text-2);
  transition: background var(--dur-base) var(--ease), color var(--dur-base) var(--ease);
}

.status-pill:hover {
  background: var(--surface-3);
}

.status-pill.pill-ok {
  background: var(--ok-soft);
  color: var(--ok);
}

.status-pill.pill-warn {
  background: var(--warn-soft);
  color: var(--warn);
}

.status-pill.pill-danger {
  background: var(--danger-soft);
  color: var(--danger);
}
</style>
