<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from "vue";
import { useApp } from "../../stores/app";
import { useLogs } from "../../stores/logs";
import type { LogFilter } from "../../types";
import AppIcon from "../ui/AppIcon.vue";
import LogList from "./LogList.vue";

const { drawerOpen, closeDrawer } = useApp();
const { filtered, counts, level, loading, refresh, copy, clear } = useLogs();

const filters: Array<{ key: LogFilter; label: string }> = [
  { key: "all", label: "全部" },
  { key: "info", label: "信息" },
  { key: "warn", label: "警告" },
  { key: "error", label: "错误" },
];

let timer: number | undefined;
const bodyRef = ref<HTMLElement | null>(null);

function stop(): void {
  window.clearInterval(timer);
  timer = undefined;
}

function onKey(event: KeyboardEvent): void {
  if (event.key === "Escape") closeDrawer();
}

function atBottom(el: HTMLElement): boolean {
  return el.scrollHeight - el.scrollTop - el.clientHeight < 40;
}

/**
 * 日志按时间正序渲染，最新一条在底部。
 * 打开抽屉、或用户原本就贴着底部时，刷新后继续贴底；用户上翻查看历史时不打断。
 */
async function load(initial = false): Promise<void> {
  const el = bodyRef.value;
  const stick = initial || !el || atBottom(el);
  await refresh();
  if (!stick) return;
  await nextTick();
  const node = bodyRef.value;
  if (node) node.scrollTop = node.scrollHeight;
}

async function setLevel(next: LogFilter): Promise<void> {
  level.value = next;
  await nextTick();
  const node = bodyRef.value;
  if (node) node.scrollTop = node.scrollHeight;
}

watch(drawerOpen, async (open) => {
  stop();
  window.removeEventListener("keydown", onKey);
  if (open) {
    window.addEventListener("keydown", onKey);
    await load(true);
    timer = window.setInterval(() => void load(), 3000);
  }
});

onBeforeUnmount(() => {
  stop();
  window.removeEventListener("keydown", onKey);
});
</script>

<template>
  <Transition name="drawer">
    <div v-if="drawerOpen" class="layer">
      <button type="button" class="scrim" aria-label="关闭日志" @click="closeDrawer" />
      <aside class="drawer" aria-label="活动日志">
        <header class="drawer-head">
          <AppIcon name="activity" />
          <h2>活动日志</h2>
          <span v-if="loading" class="spinner" aria-label="读取中" />
          <div class="drawer-actions">
            <button type="button" class="icon-btn" aria-label="复制日志" title="复制日志" @click="copy">
              <AppIcon name="copy" :size="15" />
            </button>
            <button type="button" class="icon-btn" aria-label="清空日志" title="清空日志" @click="clear">
              <AppIcon name="trash" :size="15" />
            </button>
            <button type="button" class="icon-btn" aria-label="关闭" @click="closeDrawer">
              <AppIcon name="x" :size="16" />
            </button>
          </div>
        </header>

        <div class="segmented filters">
          <button
            v-for="item in filters"
            :key="item.key"
            type="button"
            class="seg-item"
            :class="{ active: level === item.key }"
            @click="setLevel(item.key)"
          >
            {{ item.label }}
            <span class="count">{{ counts[item.key] }}</span>
          </button>
        </div>

        <div ref="bodyRef" class="drawer-body">
          <LogList :lines="filtered" />
        </div>

        <footer class="drawer-foot">只显示最近 400 行，每 3 秒刷新一次。</footer>
      </aside>
    </div>
  </Transition>
</template>

<style scoped>
.layer {
  position: absolute;
  inset: 0;
  z-index: 30;
}

.scrim {
  position: absolute;
  inset: 0;
  background: var(--scrim);
  animation: fade var(--dur-base) var(--ease);
}

.drawer {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  display: flex;
  flex-direction: column;
  width: min(430px, 86%);
  border-left: 1px solid var(--border);
  background: var(--surface);
  box-shadow: -18px 0 40px -24px rgba(var(--shadow-rgb), 0.4);
  animation: slide var(--dur-slow) var(--ease);
}

.drawer-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 13px 12px 10px 16px;
  border-bottom: 1px solid var(--border);
}

.drawer-head h2 {
  flex: 1;
  min-width: 0;
  margin: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--text-base);
  font-weight: 650;
}

.drawer-head .icon {
  color: var(--text-3);
}

.drawer-actions {
  display: flex;
  align-items: center;
  flex: none;
  gap: 2px;
  margin-left: auto;
}

.filters {
  margin: 10px 12px 6px;
}

.count {
  margin-left: 3px;
  font-size: var(--text-xs);
  color: var(--text-3);
}

.seg-item.active .count {
  color: var(--text-2);
}

.drawer-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
  -webkit-overflow-scrolling: touch;
  padding: 4px 16px 12px;
}

/* 手机端让出状态栏、横屏刘海与手势条 */
:global(html[data-platform="android"] .drawer) {
  padding-top: var(--safe-top);
  padding-right: var(--safe-right);
  padding-bottom: var(--safe-bottom);
}

.drawer-foot {
  padding: 9px 16px;
  border-top: 1px solid var(--border);
  font-size: var(--text-xs);
  color: var(--text-3);
}

@keyframes fade {
  from {
    opacity: 0;
  }
}

@keyframes slide {
  from {
    transform: translateX(18px);
    opacity: 0;
  }
}

.drawer-enter-active,
.drawer-leave-active {
  transition: opacity var(--dur-base) var(--ease);
}

.drawer-enter-from,
.drawer-leave-to {
  opacity: 0;
}
</style>
