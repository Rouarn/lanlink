<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import { useApp } from "./stores/app";
import { useSettings } from "./stores/settings";
import { useRooms } from "./stores/rooms";
import { useConnection } from "./stores/connection";
import IconSprite from "./components/ui/IconSprite.vue";
import AppBar from "./components/layout/AppBar.vue";
import ActivityDrawer from "./components/activity/ActivityDrawer.vue";
import ToastHost from "./components/ui/ToastHost.vue";
import RoomView from "./views/RoomView.vue";
import SettingsView from "./views/SettingsView.vue";

const { view, loadInfo } = useApp();
const { load: loadSettings } = useSettings();
const { loadRecent } = useRooms();
const connection = useConnection();

onMounted(async () => {
  await loadInfo();
  await Promise.allSettled([loadSettings(), loadRecent()]);
  connection.startPolling();
});

onUnmounted(() => connection.stopPolling());
</script>

<template>
  <div class="window">
    <IconSprite />
    <AppBar />
    <main class="content">
      <RoomView v-if="view === 'room'" />
      <SettingsView v-else />
    </main>
    <ActivityDrawer />
    <ToastHost />
  </div>
</template>
