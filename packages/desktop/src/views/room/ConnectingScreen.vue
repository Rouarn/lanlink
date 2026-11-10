<script setup lang="ts">
import { computed } from "vue";
import { useApp } from "../../stores/app";
import { useConnection } from "../../stores/connection";
import { useRooms } from "../../stores/rooms";
import AppIcon from "../../components/ui/AppIcon.vue";

const { openDrawer } = useApp();
const { stage, cancel } = useConnection();
const { room, preview } = useRooms();

const name = computed(() => room.value?.name || preview.value?.name || "房间");

const steps = computed(() => {
  const current = stage.value;
  return [
    { key: "resolve", label: "确认房间配置", state: current === "resolving" ? "active" : "done" },
    {
      key: "auth",
      label: "检查运行权限",
      state:
        current === "authorizing" ? "active" : current === "network" ? "done" : "todo",
    },
    {
      key: "network",
      label: "建立虚拟网络",
      state: current === "network" ? "active" : "todo",
    },
  ];
});

async function abort(): Promise<void> {
  await cancel();
}
</script>

<template>
  <section class="screen narrow connecting">
    <div class="pulse" aria-hidden="true"><span class="spinner" /></div>
    <h1>正在连接「{{ name }}」</h1>
    <p class="lede center">通常几秒钟就好。第一次连接可能会问一次管理员权限。</p>

    <ol class="steps">
      <li v-for="step in steps" :key="step.key" class="step" :class="step.state">
        <span class="step-mark">
          <AppIcon v-if="step.state === 'done'" name="check" :size="13" />
          <span v-else-if="step.state === 'active'" class="spinner" />
        </span>
        {{ step.label }}
        <span v-if="step.state === 'active'" class="step-note">进行中</span>
      </li>
    </ol>

    <div class="connecting-actions">
      <button type="button" class="link subtle" @click="abort">取消</button>
      <span class="sep" />
      <button type="button" class="link subtle" @click="openDrawer">查看活动日志</button>
    </div>
  </section>
</template>

<style scoped>
.connecting {
  padding-top: 46px;
  text-align: center;
}

.pulse {
  display: grid;
  place-items: center;
  width: 46px;
  height: 46px;
  margin: 0 auto 16px;
  border-radius: 50%;
  background: var(--accent-soft);
}

.pulse .spinner {
  width: 17px;
  height: 17px;
  border-width: 2px;
}

h1 {
  margin: 0 0 6px;
  font-size: var(--text-title);
  font-weight: 640;
  letter-spacing: -0.012em;
}

.lede.center {
  margin: 0 auto 22px;
  text-align: center;
}

.steps {
  width: 320px;
  max-width: 100%;
  margin: 0 auto;
  padding: 14px;
  list-style: none;
  border: 1px solid var(--border);
  border-radius: var(--radius-xl);
  background: var(--surface);
  text-align: left;
}

.step {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 2px;
  font-size: var(--text-base);
  color: var(--text-3);
}

.step.active {
  color: var(--text);
  font-weight: 600;
}

.step.done {
  color: var(--text-2);
}

.step-mark {
  display: grid;
  place-items: center;
  width: 18px;
  height: 18px;
  flex: none;
  border: 1px solid var(--border-strong);
  border-radius: 50%;
  color: var(--ok);
}

.step.done .step-mark {
  border-color: transparent;
  background: var(--ok-soft);
}

.step-mark .spinner {
  width: 11px;
  height: 11px;
  border-width: 1.6px;
}

.step-note {
  margin-left: auto;
  font-size: var(--text-xs);
  font-weight: 500;
  color: var(--text-3);
}

.connecting-actions {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 12px;
  margin-top: 18px;
}

.sep {
  width: 1px;
  height: 12px;
  background: var(--border-strong);
}
</style>
