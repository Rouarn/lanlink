<script setup lang="ts">
import { ref } from "vue";
import { useSettings } from "../../stores/settings";
import { useRooms } from "../../stores/rooms";
import AppIcon from "../../components/ui/AppIcon.vue";

const { saving, saveError, saveRelay } = useSettings();
const { goJoin } = useRooms();

const address = ref("");
const localError = ref("");

async function save(): Promise<void> {
  localError.value = "";
  if (!address.value.trim()) {
    localError.value = "先填一个中继地址，例如 tcp://你的服务器:11010";
    return;
  }
  await saveRelay(address.value);
}
</script>

<template>
  <section class="screen narrow">
    <p class="eyebrow">第一步</p>
    <h1 class="display">先花一分钟，把路铺好。</h1>
    <p class="lede">
      LanLink 需要一个中继服务器帮大家碰面。填一次就好，之后都在这台电脑上记住。
    </p>

    <div class="relay-diagram" aria-hidden="true">
      <span class="node">你</span>
      <span class="line" />
      <span class="node relay"><AppIcon name="server" :size="14" /> 中继</span>
      <span class="line" />
      <span class="node">朋友</span>
    </div>

    <form class="card" @submit.prevent="save">
      <label class="field">
        <span class="field-label">中继服务器</span>
        <input
          v-model="address"
          class="input mono"
          placeholder="tcp://你的服务器:11010"
          spellcheck="false"
          autocomplete="off"
        />
        <span class="field-hint">填自己或朋友搭的 EasyTier 中继地址。多条可以稍后在设置里加。</span>
      </label>
      <p v-if="localError || saveError" class="field-hint error">{{ localError || saveError }}</p>
      <div class="form-actions">
        <button class="btn btn-primary btn-lg" type="submit" :disabled="saving">
          {{ saving ? "保存中…" : "保存并继续" }}
          <AppIcon v-if="!saving" name="arrow-right" />
        </button>
        <button class="link subtle" type="button" @click="goJoin">
          我已经有房间码
          <AppIcon name="chevron-right" :size="13" />
        </button>
      </div>
    </form>

    <p class="footnote">
      没有自己的中继？让已经有房间的朋友把「完整房间码」发来，里面的配置够你直接加入。
    </p>
  </section>
</template>

<style scoped>
.relay-diagram {
  display: flex;
  align-items: center;
  gap: 10px;
  margin: 24px 0 20px;
  color: var(--text-3);
}

.node {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 28px;
  padding: 0 11px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: var(--surface);
  font-size: var(--text-sm);
  font-weight: 600;
  color: var(--text-2);
}

.node.relay {
  border-color: var(--accent-soft-2);
  background: var(--accent-soft);
  color: var(--accent);
}

.line {
  flex: 1;
  height: 1px;
  background-image: linear-gradient(to right, var(--border-strong) 50%, transparent 50%);
  background-size: 6px 1px;
}
</style>
