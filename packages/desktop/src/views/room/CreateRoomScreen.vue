<script setup lang="ts">
import { useApp } from "../../stores/app";
import { useRooms } from "../../stores/rooms";
import { useSettings } from "../../stores/settings";
import AppIcon from "../../components/ui/AppIcon.vue";
import RoomTypePicker from "../../components/room/RoomTypePicker.vue";
import RecentRoomsPanel from "./RecentRoomsPanel.vue";

const { go } = useApp();
const { roomName, creating, create, goJoin } = useRooms();
const { configured, relays } = useSettings();
</script>

<template>
  <section class="screen">
    <p class="eyebrow">房间</p>
    <h1>开一个房间</h1>
    <p class="lede">取个名字，拿到房间码，发给朋友就能进来。整个过程都在本地完成。</p>

    <div class="card">
      <RoomTypePicker />

      <label class="field">
        <span class="field-label">房间名</span>
        <input
          v-model="roomName"
          class="input"
          placeholder="随便填，例如 周五开黑"
          maxlength="24"
          spellcheck="false"
        />
      </label>

      <div v-if="configured" class="readiness ok">
        <span class="dot ok" />
        中继已就绪
        <span class="mono relay-address">{{ relays[0] }}</span>
      </div>
      <div v-else class="readiness warn">
        <span class="dot warn" />
        还没有中继服务器，创建房间前要先填一条。
        <button type="button" class="link small" @click="go('settings')">去设置</button>
      </div>

      <div class="form-actions create-actions">
        <button
          type="button"
          class="btn btn-primary btn-lg"
          :disabled="!configured || creating"
          @click="create"
        >
          {{ creating ? "正在创建…" : "创建并连接" }}
          <AppIcon v-if="!creating" name="arrow-right" />
        </button>
        <button type="button" class="link subtle" @click="goJoin">
          朋友给了房间码
          <AppIcon name="chevron-right" :size="13" />
        </button>
      </div>
    </div>

    <RecentRoomsPanel />
  </section>
</template>

<style scoped>
h1 {
  margin: 0 0 6px;
  font-size: var(--text-title);
  font-weight: 660;
  letter-spacing: -0.012em;
}

.readiness {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  padding: 8px 10px;
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
}

.readiness.ok {
  background: var(--ok-soft);
  color: var(--ok);
}

.readiness.warn {
  background: var(--warn-soft);
  color: var(--warn);
}

.relay-address {
  min-width: 0;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--text-xs);
  opacity: 0.75;
}

.create-actions {
  margin-top: 16px;
}
</style>
