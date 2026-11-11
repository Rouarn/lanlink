<script setup lang="ts">
import { computed } from "vue";
import { useApp } from "../../../stores/app";
import { useConnection } from "../../../stores/connection";
import { useSettings } from "../../../stores/settings";
import { lanIssue } from "../../../lib/lan";
import AppSwitch from "../../ui/AppSwitch.vue";
import AppIcon from "../../ui/AppIcon.vue";

const { toast } = useApp();
const { phase, status, applyStatus } = useConnection();
const { lanDiscovery, lanBusy, setLanDiscovery } = useSettings();

const connected = computed(() => phase.value === "connected");
const running = computed(() => Boolean(status.value?.lan_relay));
const issue = computed(() => lanIssue(status.value));

const dotTone = computed(() => {
  if (issue.value?.level === "error") return "danger";
  if (issue.value?.level === "warn") return "warn";
  return running.value ? "ok" : "idle";
});

const note = computed(() => {
  if (!lanDiscovery.value) return "已关闭，不会广播任何东西。";
  if (running.value) return "正在运行 · 224.0.2.60:4445";
  return connected.value ? "正在等待启动，通常几秒内就绪。" : "连上房间后自动开始。";
});

async function toggle(next: boolean): Promise<void> {
  const nextStatus = await setLanDiscovery(next);
  if (nextStatus) applyStatus(nextStatus);
  toast(next ? "已开启我的世界局域网发现" : "已关闭我的世界局域网发现");
}
</script>

<template>
  <div class="panel">
    <div class="panel-head">
      <span class="panel-title">我的世界 · Java 版</span>
      <AppSwitch :model-value="lanDiscovery" :disabled="lanBusy" label="我的世界局域网发现" @update:model-value="toggle" />
    </div>

    <div class="status-line">
      <span class="dot" :class="dotTone" />
      {{ note }}
    </div>

    <p v-if="issue" class="field-hint" :class="issue.level === 'info' ? '' : issue.level">
      <template v-if="issue.level === 'info'">{{ issue.message }}</template>
      <template v-else-if="issue.level === 'warn'">
        部分功能受限：{{ issue.message }}。发现服务仍在运行，只是其中一项没起来。
      </template>
      <template v-else>
        发现服务没起来：{{ issue.message }}。可以关掉再打开试一次，或直接用上面的虚拟 IP 连。
      </template>
    </p>

    <div class="support-list">
      <p class="support yes">
        <AppIcon name="check" />
        <span><strong>支持</strong>：Java 版「对局域网开放」，自动出现在朋友的多人生存列表里</span>
      </p>
      <p class="support no">
        <AppIcon name="x" />
        <span><strong>不支持</strong>：基岩版、网易版，它们不走这个协议</span>
      </p>
      <p class="support no">
        <AppIcon name="x" />
        <span><strong>不一定</strong>：带自定义联机 mod 的整合包，可能改了发现方式</span>
      </p>
    </div>

    <p class="panel-note">
      只影响「局域网列表里能不能看见」。看不见时，用上面的虚拟 IP 直接连也一样能玩。
    </p>
  </div>
</template>

<style scoped>
.panel-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 10px;
}

.panel-title {
  font-size: var(--text-base);
  font-weight: 650;
}

.panel-note {
  margin: 10px 0 0;
  font-size: var(--text-xs);
  line-height: 1.6;
  color: var(--text-3);
}
</style>
