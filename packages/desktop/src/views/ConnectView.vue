<script setup lang="ts">
import BannerNotice from "../components/ui/BannerNotice.vue";
import RoomComposer from "../components/connect/RoomComposer.vue";
import PreviewCard from "../components/connect/PreviewCard.vue";
import ActiveRoomCard from "../components/connect/ActiveRoomCard.vue";
import RecentRoomsCard from "../components/connect/RecentRoomsCard.vue";
import { useRooms } from "../composables/useRooms";
import { useSettings } from "../composables/useSettings";
import { useTunnel } from "../composables/useTunnel";
import { activeTab, busy, message } from "../stores/app";

const { room, recent, preview, activeRoom, resetActiveRoom } = useRooms();
const { configured } = useSettings();
const { status, state, connect, disconnect } = useTunnel();

/** 断开连接：停 core、回到未连接；最近房间记录保留。 */
async function handleDisconnect(): Promise<void> {
  await disconnect();
  resetActiveRoom();
}
</script>

<template>
  <BannerNotice
    v-if="!configured"
    title="尚未配置中继服务器"
    text="创建房间前需要先在设置里填写至少一条中继地址；加入别人的房间不用配置，房间码里自带中继。"
  >
    <template #action>
      <button class="banner-action" @click="activeTab = 'settings'">去设置</button>
    </template>
  </BannerNotice>

  <RoomComposer />

  <PreviewCard v-if="preview" :preview="preview" />

  <ActiveRoomCard
    v-if="activeRoom"
    :active-room="activeRoom"
    :exportable="Boolean(room)"
    @disconnect="handleDisconnect"
  />

  <RecentRoomsCard v-if="recent.length" :rooms="recent" />

  <BannerNotice
    v-if="state === 'failed'"
    :title="status?.error ?? '连接失败'"
    :text="status?.hint"
  >
    <template #action>
      <button v-if="!busy && activeRoom" class="banner-action" @click="connect(activeRoom.code)">
        重试
      </button>
    </template>
  </BannerNotice>

  <p v-if="message" class="footnote">{{ message }}</p>
</template>
