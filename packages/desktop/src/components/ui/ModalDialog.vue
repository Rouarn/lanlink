<script setup lang="ts">
import { onBeforeUnmount, watch } from "vue";
import AppIcon from "./AppIcon.vue";

const open = defineModel<boolean>({ required: true });
const props = withDefaults(defineProps<{ title: string; width?: number }>(), { width: 440 });

function onKey(event: KeyboardEvent): void {
  if (event.key === "Escape") open.value = false;
}

watch(
  open,
  (value) => {
    if (value) window.addEventListener("keydown", onKey);
    else window.removeEventListener("keydown", onKey);
  },
  { immediate: true },
);

onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <Teleport to="body">
    <div v-if="open" class="scrim modal-scrim" @click.self="open = false">
      <section
        class="dialog"
        role="dialog"
        aria-modal="true"
        :aria-label="props.title"
        :style="{ width: `${props.width}px` }"
      >
        <header class="dialog-head">
          <h2>{{ props.title }}</h2>
          <button class="icon-btn" type="button" aria-label="关闭" @click="open = false">
            <AppIcon name="x" />
          </button>
        </header>
        <div class="dialog-body"><slot /></div>
        <footer v-if="$slots.footer" class="dialog-foot"><slot name="footer" /></footer>
      </section>
    </div>
  </Teleport>
</template>

<style scoped>
.scrim {
  position: fixed;
  inset: 0;
  z-index: 40;
  display: grid;
  /* 定宽弹窗在窄屏靠 100% 收缩；列必须是 minmax(0,1fr)，否则 auto 列会被内容撑开 */
  grid-template-columns: minmax(0, 1fr);
  place-items: center;
  padding: 32px;
  background: var(--scrim);
  -webkit-backdrop-filter: blur(3px);
  backdrop-filter: blur(3px);
  animation: fade var(--dur-base) var(--ease);
}

.dialog {
  max-width: 100%;
  max-height: 86vh;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--border);
  border-radius: var(--radius-window);
  background: var(--surface);
  box-shadow: 0 24px 60px -18px rgba(var(--shadow-rgb), 0.42);
  animation: rise var(--dur-slow) var(--ease);
}

.dialog-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 14px 14px 10px 18px;
}

.dialog-head h2 {
  margin: 0;
  font-size: var(--text-lg);
  font-weight: 650;
}

.dialog-body {
  min-height: 0;
  padding: 4px 18px 16px;
  overflow-y: auto;
  overscroll-behavior: contain;
  -webkit-overflow-scrolling: touch;
  font-size: var(--text-base);
  color: var(--text-2);
}

.dialog-foot {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 8px;
  padding: 12px 18px 16px;
  border-top: 1px solid var(--border);
}

@keyframes fade {
  from {
    opacity: 0;
  }
}

@keyframes rise {
  from {
    opacity: 0;
    transform: translateY(8px) scale(0.985);
  }
}
</style>
