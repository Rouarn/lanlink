<script setup lang="ts">
import type { LogLine } from "../../types";

defineProps<{ lines: LogLine[] }>();

function levelClass(level: string): string {
  const value = level.toLowerCase();
  if (value.startsWith("err") || value === "fatal" || value === "panic") return "error";
  if (value.startsWith("warn")) return "warn";
  return "info";
}
</script>

<template>
  <ul v-if="lines.length" class="log-list">
    <li v-for="(line, index) in lines" :key="`${line.time}-${index}`" class="log-row">
      <span class="log-time mono">{{ line.time }}</span>
      <span class="log-level" :class="levelClass(line.level)">{{ line.level.toUpperCase() }}</span>
      <span class="log-msg">{{ line.message }}</span>
    </li>
  </ul>
  <p v-else class="empty">这里还没有日志。连接房间后会有记录。</p>
</template>

<style scoped>
.log-list {
  margin: 0;
  padding: 0;
  list-style: none;
}

.log-row {
  display: grid;
  grid-template-columns: 62px 42px minmax(0, 1fr);
  gap: 8px;
  padding: 5px 0;
  border-bottom: 1px solid var(--border);
  font-size: var(--text-xs);
  line-height: 1.55;
}

.log-time {
  color: var(--text-3);
  white-space: nowrap;
}

.log-level {
  font-weight: 650;
  letter-spacing: 0.02em;
}

.log-level.info {
  color: var(--text-3);
}

.log-level.warn {
  color: var(--warn);
}

.log-level.error {
  color: var(--danger);
}

.log-msg {
  color: var(--text-2);
  word-break: break-word;
}
</style>
