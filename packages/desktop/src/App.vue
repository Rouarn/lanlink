<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import AppHeader from "./components/layout/AppHeader.vue";
import AppSidebar from "./components/layout/AppSidebar.vue";
import ConnectView from "./views/ConnectView.vue";
import LogsView from "./views/LogsView.vue";
import PeersView from "./views/PeersView.vue";
import SettingsView from "./views/SettingsView.vue";
import { useRooms } from "./composables/useRooms";
import { useSettings } from "./composables/useSettings";
import { useTunnel } from "./composables/useTunnel";
import { activeTab, loadAppInfo, info } from "./stores/app";

const { refresh, startPolling, stopPolling } = useTunnel();
const { loadSettings } = useSettings();
const { loadRecent } = useRooms();

onMounted(async () => {
  await loadAppInfo();
  // Windows 没有 macOS 的 vibrancy 材质，半透明区域会透出桌面，
  // 把平台标到 <html> 上，由 CSS 给 Windows 补不透明背景
  if (info.value?.platform) {
    document.documentElement.dataset.platform = info.value.platform;
  }
  await loadSettings();
  await loadRecent();
  await refresh();
  startPolling();
});

onUnmounted(stopPolling);
</script>

<template>
  <div class="window">
    <AppSidebar />

    <main class="content">
      <AppHeader />

      <div class="content-body">
        <ConnectView v-if="activeTab === 'connect'" />
        <PeersView v-else-if="activeTab === 'peers'" />
        <LogsView v-else-if="activeTab === 'logs'" />
        <SettingsView v-else />
      </div>
    </main>
  </div>
</template>
