<script setup lang="ts">
import { useSettings } from "../../stores/settings";
import AppIcon from "../ui/AppIcon.vue";

const {
  resolverUrl,
  defaultResolver,
  resolverEffective,
  resolverIsDefault,
  dirty,
  saving,
  saveError,
  save,
  markDirty,
} = useSettings();

async function commit(): Promise<void> {
  await save();
}

async function restore(): Promise<void> {
  resolverUrl.value = "";
  await save();
}
</script>

<template>
  <section class="card">
    <header class="card-head">
      <AppIcon name="link" />
      <h2>解析服务</h2>
      <span class="chip push">{{ resolverIsDefault ? "内置" : "自定义" }}</span>
    </header>

    <label class="field">
      <span class="field-label">短码解析地址</span>
      <input
        v-model="resolverUrl"
        class="input mono"
        :placeholder="defaultResolver"
        spellcheck="false"
        autocomplete="off"
        @input="markDirty"
      />
      <span class="field-hint">留空就用内置地址。只有短码需要它，完整码不经过服务器。</span>
    </label>

    <div class="kv effective">
      <span>实际使用</span>
      <span class="mono">{{ resolverEffective || "—" }}</span>
    </div>

    <p v-if="saveError" class="field-hint error">{{ saveError }}</p>

    <div class="form-actions">
      <button type="button" class="btn btn-secondary btn-sm" :disabled="saving || !dirty" @click="commit">
        {{ saving ? "保存中…" : "保存" }}
      </button>
      <button
        type="button"
        class="btn btn-ghost btn-sm"
        :disabled="saving || resolverIsDefault"
        @click="restore"
      >
        恢复默认
      </button>
    </div>
  </section>
</template>

<style scoped>
.push {
  margin-left: auto;
}

.effective {
  padding: 8px 0 2px;
  font-size: var(--text-sm);
}

.effective .mono {
  font-size: var(--text-xs);
  max-width: 58%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
