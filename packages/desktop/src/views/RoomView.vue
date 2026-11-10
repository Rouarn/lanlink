<script setup lang="ts">
import { computed } from "vue";
import { useApp } from "../stores/app";
import { useConnection } from "../stores/connection";
import OnboardingScreen from "./room/OnboardingScreen.vue";
import CreateRoomScreen from "./room/CreateRoomScreen.vue";
import JoinRoomScreen from "./room/JoinRoomScreen.vue";
import ConnectingScreen from "./room/ConnectingScreen.vue";
import ConnectedScreen from "./room/ConnectedScreen.vue";
import FailedScreen from "./room/FailedScreen.vue";

const { roomScreen } = useApp();
const { phase } = useConnection();

const screens = {
  onboarding: OnboardingScreen,
  create: CreateRoomScreen,
  join: JoinRoomScreen,
  connecting: ConnectingScreen,
  connected: ConnectedScreen,
  failed: FailedScreen,
};

/** 连接状态优先：连接中/已连接/失败时，内部入口不再抢画面 */
const screen = computed<keyof typeof screens>(() => {
  if (phase.value === "connected") return "connected";
  if (phase.value === "connecting") return "connecting";
  if (phase.value === "error") return "failed";
  if (phase.value === "unconfigured" && roomScreen.value === "create") return "onboarding";
  return roomScreen.value;
});
</script>

<template>
  <component :is="screens[screen]" />
</template>
