<script setup lang="ts">
import { computed, ref } from "vue";
import { useApp } from "../../../stores/app";
import { useConnection } from "../../../stores/connection";
import {
  DEFAULT_BROADCAST_PORTS,
  useSettings,
} from "../../../stores/settings";
import AppSwitch from "../../ui/AppSwitch.vue";
import AppIcon from "../../ui/AppIcon.vue";
import { lanIssue } from "../../../lib/lan";

const { toast } = useApp();
const { phase, status, applyStatus } = useConnection();
const { broadcastPorts, lanBusy, lanError, setBroadcastPorts, validatePort } = useSettings();

const input = ref("");
const inputError = ref("");
const remembered = ref<number[]>([...DEFAULT_BROADCAST_PORTS]);

const on = computed(() => broadcastPorts.value.length > 0);
const connected = computed(() => phase.value === "connected");
const issue = computed(() => lanIssue(status.value));

const dotTone = computed(() => {
  if (issue.value?.level === "error") return "danger";
  if (issue.value?.level === "warn") return "warn";
  return on.value ? "ok" : "idle";
});

const note = computed(() => {
  if (!on.value) return "已关闭，端口表为空。";
  if (connected.value) return `正在转发 ${broadcastPorts.value.length} 个端口`;
  return `已配置 ${broadcastPorts.value.length} 个端口，连上房间后转发`;
});

async function applyPorts(next: number[]): Promise<void> {
  const nextStatus = await setBroadcastPorts(next);
  if (nextStatus) applyStatus(nextStatus);
}

async function toggle(next: boolean): Promise<void> {
  if (next) {
    const restore = remembered.value.length ? remembered.value : DEFAULT_BROADCAST_PORTS;
    await applyPorts(restore);
    toast("已开启通用 UDP 广播转发");
  } else {
    remembered.value = [...broadcastPorts.value];
    await applyPorts([]);
    toast("已关闭通用 UDP 广播转发");
  }
}

async function add(): Promise<void> {
  inputError.value = validatePort(input.value);
  if (inputError.value) return;
  await applyPorts([...broadcastPorts.value, Number(input.value)].sort((a, b) => a - b));
  input.value = "";
}

async function remove(port: number): Promise<void> {
  await applyPorts(broadcastPorts.value.filter((item) => item !== port));
}
</script>

<template>
  <div class="panel">
    <div class="panel-head">
      <span class="panel-title">通用 UDP 广播转发</span>
      <AppSwitch :model-value="on" :disabled="lanBusy" label="通用 UDP 广播转发" @update:model-value="toggle" />
    </div>

    <div class="status-line">
      <span class="dot" :class="dotTone" />
      {{ note }}
    </div>

    <p v-if="issue" class="field-hint" :class="issue.level">
      <template v-if="issue.level === 'warn'">
        部分功能受限：{{ issue.message }}。其余端口仍在转发。
      </template>
      <template v-else>
        广播转发没起来：{{ issue.message }}。可以关掉再打开试一次，或直接连对方的虚拟 IP。
      </template>
    </p>

    <div v-if="on" class="ports">
      <span v-for="port in broadcastPorts" :key="port" class="port-chip mono">
        {{ port }}
        <button type="button" :aria-label="`移除端口 ${port}`" @click="remove(port)">
          <AppIcon name="x" :size="11" />
        </button>
      </span>
    </div>

    <div class="port-add">
      <input
        v-model="input"
        class="input input-sm mono port-input"
        placeholder="添加端口，例如 27015"
        inputmode="numeric"
        @keyup.enter="add"
      />
      <button type="button" class="btn btn-secondary btn-sm" :disabled="lanBusy || !input.trim()" @click="add">
        <AppIcon name="plus" :size="14" />
        添加
      </button>
    </div>
    <p v-if="inputError" class="field-hint error">{{ inputError }}</p>
    <p v-if="lanError" class="field-hint error">{{ lanError }}</p>

    <div class="support-list">
      <p class="support yes">
        <AppIcon name="check" />
        <span><strong>支持</strong>：CS 1.6 / 半条命、魔兽争霸 III、帝国时代、虚幻竞技场、Quake III</span>
      </p>
      <p class="support no">
        <AppIcon name="x" />
        <span><strong>不支持</strong>：只走 TCP 广播的游戏，以及自己实现发现协议、不走广播的游戏</span>
      </p>
    </div>

    <p class="panel-note">
      表里是这些游戏常见的局域网端口，自定义端口自己加。端口表清空就等于关闭。
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

.ports {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
  margin-top: 10px;
}

.port-chip {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  height: 22px;
  padding: 0 5px 0 7px;
  border-radius: var(--radius-sm);
  background: var(--surface-2);
  font-size: var(--text-xs);
  color: var(--text-2);
}

.port-chip button {
  display: grid;
  place-items: center;
  width: 15px;
  height: 15px;
  border-radius: 3px;
  color: var(--text-3);
}

.port-chip button:hover {
  background: var(--hover);
  color: var(--text);
}

.port-add {
  display: flex;
  gap: 6px;
  margin-top: 10px;
}

.port-input {
  flex: 1;
  max-width: 190px;
}

.panel-note {
  margin: 10px 0 0;
  font-size: var(--text-xs);
  line-height: 1.6;
  color: var(--text-3);
}
</style>
