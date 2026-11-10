<script setup lang="ts">
import { computed } from "vue";
import { useApp } from "../../stores/app";
import { useConnection } from "../../stores/connection";
import { useRooms } from "../../stores/rooms";
import AppIcon from "../../components/ui/AppIcon.vue";

const { openDrawer } = useApp();
const { error, dismissError, disconnect, busy } = useConnection();
const { retry, goJoin, goCreate } = useRooms();

const title = computed(() => error.value?.title ?? "这次没连上");
const detail = computed(() => error.value?.detail ?? "再试一次通常就好了。");
const technical = computed(() => error.value?.technical ?? "");

async function retryNow(): Promise<void> {
  // 先把上一次失败留下的 core 收掉（后端 start() 里也有兜底回收），再重新连接。
  // 断开期间不清错误：失败页保持可见，按钮换成「正在重新连接…」，不会闪回房间页。
  await disconnect();
  await retry();
}

function switchToJoin(): void {
  // 必须清掉失败态，否则 RoomView 里「连接失败」的优先级更高，点了也停在原页。
  dismissError();
  goJoin();
}

function back(): void {
  dismissError();
  goCreate();
}
</script>

<template>
  <section class="screen narrow failed">
    <div class="badge" aria-hidden="true"><AppIcon name="alert" :size="22" /></div>
    <h1 class="display">{{ title }}</h1>
    <p class="lede center">{{ detail }}</p>

    <div class="actions">
      <button
        type="button"
        class="btn btn-primary btn-lg"
        :disabled="busy"
        @click="retryNow"
      >
        {{ busy ? "正在重新连接…" : "再试一次" }}
        <AppIcon v-if="!busy" name="refresh" />
      </button>
      <button
        type="button"
        class="btn btn-secondary btn-lg"
        :disabled="busy"
        @click="switchToJoin"
      >
        改用完整房间码
      </button>
    </div>

    <details v-if="technical" class="tech">
      <summary>技术信息</summary>
      <pre class="mono">{{ technical }}</pre>
    </details>

    <div class="foot">
      <button type="button" class="link subtle" @click="openDrawer">查看活动日志</button>
      <span class="sep" />
      <button type="button" class="link subtle" @click="back">返回房间页</button>
    </div>
  </section>
</template>

<style scoped>
.failed {
  padding-top: 40px;
  text-align: center;
}

.failed .display {
  font-size: 26px;
  font-family: var(--font-display);
  margin-bottom: 8px;
}

.badge {
  display: grid;
  place-items: center;
  width: 46px;
  height: 46px;
  margin: 0 auto 16px;
  border-radius: 50%;
  background: var(--danger-soft);
  color: var(--danger);
}

.lede.center {
  margin: 0 auto 22px;
  text-align: center;
}

.actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 10px;
}

.tech {
  width: 360px;
  max-width: 100%;
  margin: 22px auto 0;
  text-align: left;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--surface);
  font-size: var(--text-sm);
}

.tech summary {
  padding: 9px 12px;
  color: var(--text-3);
}

.tech pre {
  margin: 0;
  padding: 0 12px 12px;
  font-size: var(--text-xs);
  line-height: 1.6;
  color: var(--text-2);
  white-space: pre-wrap;
  word-break: break-word;
}

.foot {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 12px;
  margin-top: 20px;
}

.sep {
  width: 1px;
  height: 12px;
  background: var(--border-strong);
}
</style>
