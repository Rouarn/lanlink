<script setup lang="ts">
import { ref } from "vue";
import { useRooms } from "../../stores/rooms";
import { formatRelative } from "../../lib/format";
import { roleLabel } from "../../lib/labels";
import AppIcon from "../../components/ui/AppIcon.vue";
import ModalDialog from "../../components/ui/ModalDialog.vue";

const { recent, reconnect, remove, clearRecent } = useRooms();
const expanded = ref(false);
const confirming = ref(false);

async function confirmClear(): Promise<void> {
  await clearRecent();
  confirming.value = false;
  expanded.value = false;
}
</script>

<template>
  <section v-if="recent.length" class="recent">
    <header class="recent-head">
      <button type="button" class="recent-toggle" @click="expanded = !expanded">
        <AppIcon :name="expanded ? 'chevron-down' : 'chevron-right'" :size="14" />
        最近房间
        <span class="count">{{ recent.length }}</span>
      </button>
      <button v-if="expanded" type="button" class="link small" @click="confirming = true">清空</button>
    </header>

    <ul v-if="expanded" class="recent-list">
      <li v-for="item in recent" :key="item.code" class="recent-row">
        <div class="recent-main">
          <span class="recent-name">{{ item.name }}</span>
          <span class="recent-meta mono">
            {{ item.code }} · {{ roleLabel[item.role] }} · {{ formatRelative(item.last_used) }}
          </span>
        </div>
        <div class="recent-actions">
          <button type="button" class="btn btn-secondary btn-sm" @click="reconnect(item)">连接</button>
          <button type="button" class="icon-btn" aria-label="删除这条记录" @click="remove(item.code)">
            <AppIcon name="trash" :size="14" />
          </button>
        </div>
      </li>
    </ul>

    <ModalDialog v-model="confirming" title="清空最近房间？" :width="380">
      <p class="confirm-text">
        只会删掉这台电脑上的记录，已经连着的房间不受影响。房间码如果没留，之后需要朋友再发一次。
      </p>
      <template #footer>
        <button type="button" class="btn btn-secondary" @click="confirming = false">取消</button>
        <button type="button" class="btn btn-danger-quiet" @click="confirmClear">清空</button>
      </template>
    </ModalDialog>
  </section>
</template>

<style scoped>
.recent {
  margin-top: 10px;
}

.recent-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 0 4px;
}

.recent-toggle {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 28px;
  font-size: var(--text-sm);
  font-weight: 600;
  color: var(--text-3);
}

.recent-toggle:hover {
  color: var(--text);
}

.count {
  display: inline-grid;
  place-items: center;
  min-width: 18px;
  height: 18px;
  padding: 0 5px;
  border-radius: 999px;
  background: var(--surface-2);
  font-size: var(--text-xs);
  color: var(--text-3);
}

.recent-list {
  margin: 6px 0 0;
  padding: 0;
  list-style: none;
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  background: var(--surface);
  overflow: hidden;
}

.recent-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 9px 10px 9px 14px;
}

.recent-row + .recent-row {
  border-top: 1px solid var(--border);
}

.recent-main {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
}

.recent-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--text-base);
  font-weight: 600;
}

.recent-meta {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--text-xs);
  color: var(--text-3);
}

.recent-actions {
  display: flex;
  align-items: center;
  flex: none;
  gap: 2px;
  margin-left: auto;
}

.confirm-text {
  margin: 0;
  line-height: 1.7;
}
</style>
