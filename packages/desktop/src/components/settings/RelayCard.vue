<script setup lang="ts">
import { ref } from "vue";
import { useSettings } from "../../stores/settings";
import AppIcon from "../ui/AppIcon.vue";
import ModalDialog from "../ui/ModalDialog.vue";

const { relays, relayText, saving, saveError, save, loaded } = useSettings();

const open = ref(false);
const draft = ref("");

function edit(): void {
  draft.value = relayText.value;
  open.value = true;
}

async function commit(): Promise<void> {
  relayText.value = draft.value;
  const ok = await save();
  if (ok) open.value = false;
}
</script>

<template>
  <section class="card">
    <header class="card-head">
      <AppIcon name="server" />
      <h2>中继服务器</h2>
      <button type="button" class="link small push" @click="edit">
        {{ relays.length ? "编辑" : "添加" }}
      </button>
    </header>

    <div v-if="relays.length" class="relay-list">
      <div v-for="(relay, index) in relays" :key="relay" class="relay-row">
        <span class="dot" :class="index === 0 ? 'ok' : 'idle'" />
        <span class="mono">{{ relay }}</span>
        <span v-if="index === 0" class="chip">首选</span>
      </div>
    </div>
    <p v-else-if="loaded" class="warn-note">
      <AppIcon name="alert" :size="14" />
      还没有中继。创建房间前必须填一条，加入别人的完整码可以暂时不填。
    </p>
    <p v-else class="muted">正在读取…</p>

    <p class="card-foot-note">按顺序尝试，第一条不通会自动换下一条。</p>

    <ModalDialog v-model="open" title="中继服务器" :width="440">
      <label class="field">
        <span class="field-label">地址</span>
        <textarea
          v-model="draft"
          class="code-input relay-input"
          rows="4"
          placeholder="tcp://你的服务器:11010"
          spellcheck="false"
        />
        <span class="field-hint">一行一个，支持 tcp:// 或 udp://，按顺序尝试。</span>
      </label>
      <p v-if="saveError" class="field-hint error">{{ saveError }}</p>
      <template #footer>
        <button type="button" class="btn btn-secondary" @click="open = false">取消</button>
        <button type="button" class="btn btn-primary" :disabled="saving" @click="commit">
          {{ saving ? "保存中…" : "保存" }}
        </button>
      </template>
    </ModalDialog>
  </section>
</template>

<style scoped>
.push {
  margin-left: auto;
}

.relay-list {
  margin-top: -4px;
}

.relay-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 0;
  font-size: var(--text-base);
}

.relay-row .mono {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.relay-row .chip {
  margin-left: auto;
}

.warn-note {
  display: flex;
  align-items: flex-start;
  gap: 7px;
  margin: 4px 0 0;
  font-size: var(--text-sm);
  line-height: 1.6;
  color: var(--warn);
}

.relay-input {
  min-height: 84px;
}
</style>
