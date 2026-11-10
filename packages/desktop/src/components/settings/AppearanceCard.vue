<script setup lang="ts">
import { useTheme, type ThemeMode } from "../../stores/theme";
import AppIcon from "../ui/AppIcon.vue";

const { mode, resolved, setMode } = useTheme();

const options: { key: ThemeMode; label: string; icon: string }[] = [
  { key: "system", label: "跟随系统", icon: "monitor" },
  { key: "light", label: "浅色", icon: "sun" },
  { key: "dark", label: "深色", icon: "moon" },
];

const hint = {
  system: "跟着系统的浅色 / 深色设置走。",
  light: "固定使用浅色，和系统设置无关。",
  dark: "固定使用深色，和系统设置无关。",
};
</script>

<template>
  <section class="card">
    <header class="card-head">
      <AppIcon name="sun" />
      <h2>外观</h2>
    </header>

    <div class="segmented theme-seg">
      <button
        v-for="item in options"
        :key="item.key"
        type="button"
        class="seg-item"
        :class="{ active: mode === item.key }"
        @click="setMode(item.key)"
      >
        <AppIcon :name="item.icon" :size="13" />
        {{ item.label }}
      </button>
    </div>

    <p class="card-foot-note">{{ hint[mode] }}当前显示：{{ resolved === "dark" ? "深色" : "浅色" }}</p>
  </section>
</template>

<style scoped>
.theme-seg {
  margin-bottom: var(--space-5);
}

.seg-item {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 5px;
}
</style>
