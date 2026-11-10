<script setup lang="ts">
import { onMounted, watch } from "vue";
import SegmentedControl from "../components/ui/SegmentedControl.vue";
import { useLogs } from "../composables/useLogs";
import { useTunnel } from "../composables/useTunnel";
import { logFilters } from "../lib/labels";

const { logs, logLevel, logBox, copied, refreshLogs, copyLogs, clearLogs } = useLogs();
const { running } = useTunnel();

onMounted(() => {
  void refreshLogs();
});

watch(logLevel, () => {
  void refreshLogs();
});
</script>

<template>
  <div class="log-page">
    <div class="log-toolbar">
      <SegmentedControl v-model="logLevel" :items="logFilters" />
      <div class="toolbar-actions">
        <button class="btn-mini" :disabled="!logs.length" @click="copyLogs">
          {{ copied ? "已复制" : "复制" }}
        </button>
        <button class="btn-mini" :disabled="!logs.length" @click="clearLogs">清空</button>
      </div>
    </div>

    <div ref="logBox" class="log-panel">
      <div v-if="!logs.length" class="empty">
        {{ running ? "（暂无日志）" : "连接后这里会显示 core 的输出" }}
      </div>
      <div v-for="(line, index) in logs" :key="index" class="log-line">
        <span v-if="line.time" class="log-time mono">{{ line.time }}</span>
        <span v-if="line.level" class="log-level" :class="line.level.toLowerCase()">
          {{ line.level }}
        </span>
        <span class="log-msg">{{ line.message }}</span>
      </div>
    </div>
  </div>
</template>
