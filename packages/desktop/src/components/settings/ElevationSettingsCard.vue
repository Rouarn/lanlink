<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import InfoRow from "../ui/InfoRow.vue";
import SectionCard from "../ui/SectionCard.vue";
import { ipc } from "../../lib/ipc";
import { info } from "../../stores/app";
import type { ElevationInfo } from "../../types";

const elevation = ref<ElevationInfo | null>(null);
const pending = ref(false);
const error = ref("");

/** 只有 Windows 才有「计划任务静默提权」这一档，macOS 固定启动即提权 */
const isWindows = computed(() => info.value?.platform === "windows");
const modeLabel = computed(() => {
  if (!elevation.value) return "读取中…";
  return elevation.value.silent ? "计划任务静默提权（不再弹 UAC）" : "点「连接」时请求授权";
});

async function load(): Promise<void> {
  try {
    elevation.value = await ipc.elevationInfo();
  } catch (err) {
    error.value = String(err);
  }
}

async function toggle(): Promise<void> {
  if (!elevation.value || pending.value) return;
  pending.value = true;
  error.value = "";
  try {
    elevation.value = await ipc.setSilentElevate(!elevation.value.silent);
  } catch (err) {
    error.value = String(err);
    await load();
  } finally {
    pending.value = false;
  }
}

onMounted(load);
</script>

<template>
  <SectionCard v-if="isWindows" title="提权方式">
    <InfoRow label="当前模式">
      <span class="row-value">{{ modeLabel }}</span>
    </InfoRow>
    <InfoRow label="计划任务">
      <span class="row-value">
        {{ elevation?.task_present ? "已创建（LanLink）" : "未创建" }}
      </span>
    </InfoRow>
    <InfoRow>
      <div class="row-actions">
        <button class="btn-secondary small" :disabled="pending || !elevation" @click="toggle">
          {{
            pending
              ? "处理中…"
              : elevation?.silent
                ? "取消静默提权"
                : "开启免 UAC 静默提权"
          }}
        </button>
      </div>
    </InfoRow>
    <span class="row-hint">
      开启后会创建一个名为 LanLink 的计划任务（以最高权限运行），之后启动不再弹 UAC；
      取消会删除该任务并回到「点连接时授权」。两条路径都只影响提权方式，不影响房间与中继配置。
    </span>
    <span v-if="error" class="footnote error">{{ error }}</span>
  </SectionCard>
</template>
