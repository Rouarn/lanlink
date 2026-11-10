<script setup lang="ts">
import { useApp } from "../../stores/app";
import AppIcon from "../ui/AppIcon.vue";
import StatusPill from "./StatusPill.vue";

const { view, go, goRoom, openDrawer } = useApp();
</script>

<template>
  <header class="appbar" data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region>
      <span class="brand-mark"><AppIcon name="brand" :size="17" /></span>
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
  gap: 18px;
  height: 48px;
  padding-right: 12px;
  flex: none;
  border-bottom: 1px solid var(--border);
  background: transparent;
  user-select: none;
  -webkit-user-select: none;
}

.brand {
  display: flex;
  align-items: center;
  gap: 7px;
  flex: none;
}

.brand-mark {
  display: grid;
  place-items: center;
  width: 24px;
  height: 24px;
  border-radius: 7px;
  background: var(--accent-soft);
  color: var(--accent);
}

.brand-name {
  font-size: var(--text-base);
  font-weight: 700;
  letter-spacing: -0.01em;
}

.nav {
  display: flex;
  align-items: center;
  gap: 2px;
}

.nav-item {
  height: 27px;
  padding: 0 11px;
  border-radius: var(--radius-sm);
  font-size: var(--text-base);
  font-weight: 600;
  color: var(--text-3);
  transition: background var(--dur-base) var(--ease), color var(--dur-base) var(--ease);
}

.nav-item:hover {
  background: var(--hover);
  color: var(--text-2);
}

.nav-item.active {
  background: var(--surface-2);
  color: var(--text);
}

.appbar-end {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-left: auto;
}
</style>
