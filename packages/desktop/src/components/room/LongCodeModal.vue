<script setup lang="ts">
import { useApp } from "../../stores/app";
import { copyPlainText } from "../../lib/clipboard";
import AppIcon from "../ui/AppIcon.vue";
import ModalDialog from "../ui/ModalDialog.vue";

const open = defineModel<boolean>({ required: true });
const props = defineProps<{ code: string; error: string }>();

const { toast } = useApp();

async function copy(): Promise<void> {
  if (!props.code) return;
  await copyPlainText(props.code);
  toast("完整房间码已复制");
}
</script>

<template>
  <ModalDialog v-model="open" title="完整房间码" :width="460">
    <p class="hint">
      完整码里带着中继和房间配置，朋友粘进去就能加入，不需要解析服务。适合对方没中继、或解析服务抽风的时候。
    </p>
    <textarea class="code-input long" readonly :value="code" rows="5" />
    <p v-if="error" class="field-hint error">{{ error }}</p>
    <template #footer>
      <button type="button" class="btn btn-secondary" @click="open = false">关闭</button>
      <button type="button" class="btn btn-primary" :disabled="!code" @click="copy">
        <AppIcon name="copy" :size="14" />
        复制完整码
      </button>
    </template>
  </ModalDialog>
</template>

<style scoped>
.hint {
  margin: 0 0 10px;
  font-size: var(--text-sm);
  line-height: 1.65;
  color: var(--text-3);
}

.long {
  font-size: var(--text-xs);
}
</style>
