<script setup lang="ts">
import { useApp } from "../../stores/app";
import { useCopyFeedback } from "../../composables/useClipboard";
import AppIcon from "../ui/AppIcon.vue";

const props = defineProps<{ ip: string | null }>();
const emit = defineEmits<{ why: [] }>();

const { toast } = useApp();
const { copied, copy } = useCopyFeedback();

async function copyIp(): Promise<void> {
  if (!props.ip) return;
  await copy(props.ip);
  toast("虚拟 IP 已复制");
}
</script>

<template>
  <section class="card address">
    <div class="address-head">
      <span class="field-label">你的虚拟 IP</span>
      <button type="button" class="link subtle" @click="emit('why')">
        这是什么？
        <AppIcon name="info" :size="13" />
      </button>
    </div>

    <div class="address-row">
      <strong class="ip mono">{{ ip || "正在分配…" }}</strong>
      <button type="button" class="btn btn-secondary btn-sm" :disabled="!ip" @click="copyIp">
        <AppIcon :name="copied ? 'check' : 'copy'" :size="14" />
        {{ copied ? "已复制" : "复制 IP" }}
      </button>
    </div>

    <p class="address-hint">
      游戏里用「直接连接」，填 <span class="mono">{{ ip || "10.126.126.x" }}:端口</span>。
      端口用游戏自己监听的端口，例如 CS 1.6 是 27015。
    </p>
  </section>
</template>

<style scoped>
.address {
  padding: 18px 20px;
}

.address-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.address-head .field-label {
  margin: 0;
}

.address-row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 14px;
  row-gap: 8px;
  margin: 6px 0 8px;
}

.ip {
  min-width: 0;
  overflow-wrap: anywhere;
  font-size: 25px;
  font-weight: 600;
  letter-spacing: -0.01em;
  line-height: 1.25;
}

.address-hint {
  margin: 0;
  overflow-wrap: anywhere;
  font-size: var(--text-sm);
  line-height: 1.6;
  color: var(--text-3);
}

.address-hint .mono {
  color: var(--text-2);
}
</style>
