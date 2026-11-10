<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { ipc } from "../../lib/ipc";
import { useApp } from "../../stores/app";
import type { ElevationInfo } from "../../types";
import AppIcon from "../ui/AppIcon.vue";
import AppSwitch from "../ui/AppSwitch.vue";

const { info, platform, toast } = useApp();

const elev = ref<ElevationInfo | null>(null);

const elevated = computed(() => elev.value?.elevated ?? info.value?.elevated ?? false);
const isWindows = computed(() => platform.value === "win");
const silent = computed(() => elev.value?.silent ?? false);

async function refresh(): Promise<void> {
  try {
    elev.value = await ipc.elevationInfo();
  } catch {
    toast("暂时读不到权限信息");
  }
}

async function toggleSilent(next: boolean): Promise<void> {
  try {
    elev.value = await ipc.setSilentElevate(next);
    toast(next ? "已开启静默提权" : "已关闭静默提权");
  } catch {
    toast("设置没保存成功，稍后再试");
  }
}

onMounted(refresh);
</script>

<template>
  <section class="card">
    <header class="card-head">
      <AppIcon name="shield" />
      <h2>运行权限</h2>
      <span class="chip push" :class="elevated ? 'chip-ok' : ''">
        {{ elevated ? "管理员" : "普通用户" }}
      </span>
    </header>

    <div class="status-line">
      <span class="dot" :class="elevated ? 'ok' : 'idle'" />
      <template v-if="isWindows">
        {{ elevated ? "已以管理员身份运行，虚拟网卡正常。" : "还没有管理员权限，连接时会再申请一次。" }}
      </template>
      <template v-else>
        {{ elevated ? "已获得管理员权限，可以创建虚拟网卡。" : "当前是普通用户权限，连接时会弹一次授权。" }}
      </template>
    </div>

    <template v-if="isWindows">
      <div class="elev-row">
        <div class="elev-main">
          <strong>静默提权</strong>
          <span>用计划任务在后台提权，之后连接不再弹 UAC。</span>
        </div>
        <AppSwitch :model-value="silent" label="静默提权" @update:model-value="toggleSilent" />
      </div>
      <div class="kv">
        <span>提权任务</span>
        <span>{{ elev?.task_present ? "计划任务已就绪" : "计划任务未创建" }}</span>
      </div>
    </template>

    <div v-else class="mac-note">
      <p>macOS 第一次创建虚拟网卡时会弹一次系统授权，点允许就好，之后不会重复问。</p>
      <button type="button" class="btn btn-ghost btn-sm" @click="refresh">
        <AppIcon name="refresh" :size="14" />
        重新检查
      </button>
    </div>

    <template v-if="isWindows">
      <button type="button" class="btn btn-ghost btn-sm refresh" @click="refresh">
        <AppIcon name="refresh" :size="14" />
        重新检查
      </button>
    </template>
  </section>
</template>

<style scoped>
.push {
  margin-left: auto;
}

.elev-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 14px;
  margin-top: 14px;
  padding-top: 14px;
  border-top: 1px solid var(--border);
}

.elev-main {
  display: flex;
  flex-direction: column;
  gap: 2px;
  font-size: var(--text-sm);
  color: var(--text-3);
}

.elev-main strong {
  font-size: var(--text-base);
  font-weight: 650;
  color: var(--text);
}

.mac-note {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px solid var(--border);
}

.mac-note p {
  margin: 0;
  font-size: var(--text-sm);
  line-height: 1.6;
  color: var(--text-3);
}

.refresh {
  margin-top: 10px;
}
</style>
