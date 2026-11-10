<script setup lang="ts">
import { useApp } from "../../stores/app";
import AppIcon from "../ui/AppIcon.vue";
import StatusPill from "./StatusPill.vue";

const { view, go, goRoom, openDrawer } = useApp();
</script>

<template>
  <header class="appbar" data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region>
      <AppIcon name="brand" :size="19" />
      <span class="brand-name">LanLink</span>
    </div>

    <nav class="nav" aria-label="主导航">
      <button
        type="button"
        class="nav-item"
        :class="{ active: view === 'room' }"
        @click="goRoom()"
      >
        房间
      </button>
      <button
        type="button"
        class="nav-item"
        :class="{ active: view === 'settings' }"
        @click="go('settings')"
      >
        设置
      </button>
    </nav>

    <div class="appbar-end">
      <StatusPill />
      <button type="button" class="icon-btn" aria-label="活动日志" title="活动日志" @click="openDrawer">
        <AppIcon name="activity" />
      </button>
    </div>
  </header>
</template>

<style scoped>
.appbar {
  display: flex;
  align-items: center;
  gap: 26px;
  height: 56px;
  padding-right: 20px;
  flex: none;
  border-bottom: 1px solid var(--border);
  /* 与内容区同底，避免 macOS 上顶部只剩窗口玻璃、显得比下半部分更灰 */
  background: var(--glass-content);
  user-select: none;
  -webkit-user-select: none;
}

.brand {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: none;
  color: var(--text);
}

.brand-name {
  font-size: var(--text-brand);
  font-weight: 680;
  letter-spacing: -0.01em;
}

.nav {
  display: flex;
  align-items: center;
  gap: 2px;
}

.nav-item {
  height: 28px;
  padding: 0 11px;
  border-radius: var(--radius-tight);
  font-size: var(--text-base);
  font-weight: 600;
  color: var(--text-3);
  transition: background var(--dur-base) var(--ease), color var(--dur-base) var(--ease),
    box-shadow var(--dur-base) var(--ease);
}

.nav-item:hover {
  background: var(--hover);
  color: var(--text-2);
}

.nav-item.active {
  background: var(--surface);
  color: var(--text);
  box-shadow: 0 0 0 1px var(--border), 0 1px 2px rgba(var(--shadow-rgb), 0.05);
}

.appbar-end {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-left: auto;
}
</style>
