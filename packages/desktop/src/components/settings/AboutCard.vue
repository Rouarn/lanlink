<script setup lang="ts">
import { computed } from "vue";
import { copyPlainText } from "../../lib/clipboard";
import { buildDiagnostics } from "../../lib/diagnostics";
import { useApp } from "../../stores/app";
import { useConnection } from "../../stores/connection";
import AppIcon from "../ui/AppIcon.vue";

const { info, platform, openDrawer, toast } = useApp();
const { status } = useConnection();

const platformName = computed(() => (platform.value === "win" ? "Windows" : "macOS"));

async function copyDiagnostics(): Promise<void> {
  await copyPlainText(buildDiagnostics(info.value, status.value));
  toast("诊断信息已复制");
}
</script>

<template>
  <section class="card">
    <header class="card-head">
      <AppIcon name="sparkle" />
      <h2>关于</h2>
    </header>

    <div class="kv">
      <span>版本</span>
      <span>LanLink 0.1.0 · 开发版</span>
    </div>
    <div class="kv">
      <span>平台</span>
      <span>{{ platformName }}</span>
    </div>
    <div class="kv">
      <span>内核</span>
      <span class="mono">{{ status?.version || "EasyTier 未启动" }}</span>
    </div>

    <div class="about-actions">
      <button type="button" class="btn btn-secondary btn-sm" @click="copyDiagnostics">
        <AppIcon name="copy" :size="14" />
        复制诊断信息
      </button>
      <button type="button" class="btn btn-ghost btn-sm" @click="openDrawer">
        <AppIcon name="activity" :size="14" />
        活动日志
      </button>
    </div>

    <p class="card-foot-note">给朋友用的小工具，不上传房间数据，也不做遥测。</p>
  </section>
</template>

<style scoped>
.about-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 12px;
}
</style>
