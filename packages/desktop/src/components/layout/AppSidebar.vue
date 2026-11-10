<script setup lang="ts">
import { activeTab } from "../../stores/app";
import { tabs } from "../../constants/navigation";
import { useTunnel } from "../../composables/useTunnel";
import { stateLabel } from "../../lib/labels";
import StatusDot from "../ui/StatusDot.vue";
// 品牌资源：横版 logo，浅色/暗色各一份（png 直接用，避免 svg 字体回退导致字形不对）
import logoLight from "../../assets/brand/logo-horizontal-light.png";
import logoDark from "../../assets/brand/logo-horizontal-dark.png";

const { state } = useTunnel();
</script>

<template>
  <aside class="sidebar">
    <div class="titlebar-space" data-tauri-drag-region></div>

    <div class="sidebar-brand">
      <picture>
        <source :srcset="logoDark" media="(prefers-color-scheme: dark)" />
        <img :src="logoLight" class="brand-logo" alt="VaKao" draggable="false" />
      </picture>
    </div>

    <nav class="nav">
      <button
        v-for="tab in tabs"
        :key="tab.key"
        class="nav-item"
        :class="{ active: activeTab === tab.key }"
        @click="activeTab = tab.key"
      >
        <svg
          class="nav-icon"
          viewBox="0 0 16 16"
          fill="none"
          stroke="currentColor"
          stroke-width="1.4"
          stroke-linecap="round"
          stroke-linejoin="round"
          v-html="tab.icon"
        ></svg>
        <span>{{ tab.label }}</span>
      </button>
    </nav>

    <div class="sidebar-footer">
      <StatusDot :state="state" />
      <span>{{ stateLabel[state] }}</span>
    </div>
  </aside>
</template>
