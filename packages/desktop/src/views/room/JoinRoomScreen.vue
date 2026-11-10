<script setup lang="ts">
import { watch } from "vue";
import { useApp } from "../../stores/app";
import { useRooms } from "../../stores/rooms";
import { kindLabel } from "../../lib/labels";
import AppIcon from "../../components/ui/AppIcon.vue";

const { goRoom, toast } = useApp();
const { joinCode, preview, previewCode, previewError, lookup, join } = useRooms();

watch(joinCode, (value) => {
  if (preview.value && value.trim() !== previewCode.value) preview.value = null;
});

async function paste(): Promise<void> {
  try {
    const text = await navigator.clipboard.readText();
    if (text.trim()) {
      joinCode.value = text.trim();
      toast("已粘贴房间码");
    }
  } catch {
    toast("没法读取剪贴板，手动粘贴一下");
  }
}
</script>

<template>
  <section class="screen narrow">
    <button type="button" class="link subtle back" @click="goRoom('create')">
      <AppIcon name="arrow-left" :size="13" />
      返回
    </button>

    <h1>加入房间</h1>
    <p class="lede">把朋友发来的房间码粘进来。短码会自动查配置，完整码不依赖服务器。</p>

    <div class="card">
      <label class="field">
        <span class="field-label">房间码</span>
        <textarea
          v-model="joinCode"
          class="code-input"
          placeholder="ROOM-8K3M 或 LL1. 开头的完整码"
          rows="3"
          spellcheck="false"
        />
      </label>

      <div class="form-actions">
        <button type="button" class="btn btn-secondary btn-sm" @click="paste">
          <AppIcon name="copy" :size="14" />
          从剪贴板粘贴
        </button>
        <button
          type="button"
          class="btn btn-secondary btn-sm"
          :disabled="!joinCode.trim()"
          @click="lookup"
        >
          识别房间码
        </button>
      </div>

      <p v-if="previewError" class="field-hint error join-error">
        {{ previewError.title }}。{{ previewError.detail }}
      </p>

      <div v-if="preview" class="resolve">
        <div class="resolve-head">
          <span class="chip chip-ok"><AppIcon name="check-circle" :size="12" /> 已识别</span>
          <span class="chip mono">{{ kindLabel[preview.kind] }}</span>
        </div>
        <strong class="resolve-name">{{ preview.name }}</strong>
        <div class="kv">
          <span>中继</span>
          <span class="mono">{{ preview.relays[0] || "完整码自带" }}</span>
        </div>
        <div class="kv">
          <span>身份</span>
          <span>{{ preview.role === "host" ? "房主" : "成员" }}</span>
        </div>
        <p class="footnote">
          这个房间用上面的中继组网。加入后你会拿到一个虚拟 IP，游戏里直接连它就行。
        </p>
      </div>

      <div class="form-actions join-actions">
        <button
          type="button"
          class="btn btn-primary btn-lg"
          :disabled="!preview"
          @click="join"
        >
          加入并连接
          <AppIcon name="arrow-right" />
        </button>
      </div>
    </div>

    <p class="footnote center">
      短码要经过解析服务；解析不通时，让对方发「完整房间码」，它不依赖服务器。
    </p>
  </section>
</template>

<style scoped>
.back {
  margin-bottom: 14px;
}

h1 {
  margin: 0 0 6px;
  font-size: var(--text-title);
  font-weight: 660;
  letter-spacing: -0.012em;
}

.join-error {
  margin-top: 10px;
}

.resolve {
  margin-top: 14px;
  padding: 12px;
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  background: var(--surface-2);
}

.resolve-head {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: 8px;
}

.resolve-name {
  display: block;
  margin-bottom: 4px;
  font-size: var(--text-lg);
  font-weight: 650;
}

.join-actions {
  margin-top: 16px;
}
</style>
