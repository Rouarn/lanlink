<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from "vue";
import { useApp } from "../../stores/app";
import { useRooms } from "../../stores/rooms";
import { kindLabel } from "../../lib/labels";
import { looksLikeRoomCode } from "../../lib/roomCode";
import AppIcon from "../../components/ui/AppIcon.vue";

const { goRoom, toast } = useApp();
const { joinCode, preview, previewCode, previewError, lookup, join } = useRooms();

const fromClipboard = ref(false);

watch(joinCode, (value) => {
  if (preview.value && value.trim() !== previewCode.value) preview.value = null;
  if (!value.trim()) fromClipboard.value = false;
});

/** 读剪贴板，只在看起来是房间码时返回；任何失败都当作没有，不打扰用户 */
async function readCode(): Promise<string | null> {
  try {
    const text = (await navigator.clipboard.readText()).trim();
    return looksLikeRoomCode(text) ? text : null;
  } catch {
    return null;
  }
}

/** 进页面与窗口重新获得焦点时自动读一次；已有输入绝不覆盖 */
async function autoFill(): Promise<void> {
  if (joinCode.value.trim()) return;
  const code = await readCode();
  if (!code || joinCode.value.trim()) return;
  joinCode.value = code;
  fromClipboard.value = true;
}

async function paste(): Promise<void> {
  const text = (await navigator.clipboard.readText().catch(() => "")).trim();
  if (!text) {
    toast("剪贴板里没有内容，手动粘贴一下");
    return;
  }
  joinCode.value = text;
  fromClipboard.value = false;
  toast("已粘贴房间码");
}

function onWindowFocus(): void {
  void autoFill();
}

function onVisibility(): void {
  if (!document.hidden) void autoFill();
}

onMounted(() => {
  void autoFill();
  window.addEventListener("focus", onWindowFocus);
  document.addEventListener("visibilitychange", onVisibility);
});

onUnmounted(() => {
  window.removeEventListener("focus", onWindowFocus);
  document.removeEventListener("visibilitychange", onVisibility);
});
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
        <span v-if="fromClipboard" class="field-hint clipboard-hint">已从剪贴板读取</span>
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

.clipboard-hint {
  color: var(--ok);
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
