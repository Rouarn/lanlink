<script setup lang="ts">
import { ref } from "vue";
import AppIcon from "../ui/AppIcon.vue";

const open = ref(false);
</script>

<template>
  <div class="type-select">
    <button type="button" class="type-trigger" @click="open = !open">
      <span class="type-icon"><AppIcon name="users" :size="15" /></span>
      <span class="type-text">
        <strong>标准房间</strong>
        <small>跨平台，macOS 和 Windows 都能进</small>
      </span>
      <AppIcon name="chevron-down" :size="15" />
    </button>

    <button v-if="open" type="button" class="type-dismiss" aria-label="收起" @click="open = false" />
    <div v-if="open" class="type-menu">
      <button type="button" class="type-item active" @click="open = false">
        <span class="type-item-main">
          <strong>标准房间</strong>
          <small>默认方式，基于 EasyTier 组网</small>
        </span>
        <AppIcon name="check" :size="15" />
      </button>
      <button type="button" class="type-item" disabled>
        <span class="type-item-main">
          <strong>Windows 专用</strong>
          <small>需要管理员权限和驱动，与标准房间不相通</small>
        </span>
        <span class="chip">开发中</span>
      </button>
    </div>
  </div>
</template>

<style scoped>
.type-select {
  position: relative;
  margin-bottom: 16px;
}

.type-trigger {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  background: var(--surface-2);
  text-align: left;
  transition: border-color var(--dur-base) var(--ease);
}

.type-trigger:hover {
  border-color: var(--border-strong);
}

.type-icon {
  display: grid;
  place-items: center;
  width: 28px;
  height: 28px;
  flex: none;
  border-radius: 8px;
  background: var(--surface);
  color: var(--text-2);
}

.type-text {
  display: flex;
  flex-direction: column;
  gap: 1px;
  flex: 1;
  min-width: 0;
}

.type-text strong {
  font-size: var(--text-base);
  font-weight: 650;
}

.type-text small {
  font-size: var(--text-xs);
  color: var(--text-3);
}

.type-dismiss {
  position: fixed;
  inset: 0;
  z-index: 5;
}

.type-menu {
  position: absolute;
  top: calc(100% + 5px);
  left: 0;
  right: 0;
  z-index: 6;
  padding: 4px;
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  background: var(--surface);
  box-shadow: 0 16px 36px -14px rgba(var(--shadow-rgb), 0.35);
}

.type-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  width: 100%;
  padding: 8px 9px;
  border-radius: 7px;
  text-align: left;
}

.type-item:hover:not(:disabled) {
  background: var(--hover);
}

.type-item.active {
  color: var(--accent);
}

.type-item:disabled {
  opacity: 0.55;
}

.type-item-main {
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.type-item-main strong {
  font-size: var(--text-base);
  font-weight: 650;
  color: var(--text);
}

.type-item-main small {
  font-size: var(--text-xs);
  color: var(--text-3);
}
</style>
