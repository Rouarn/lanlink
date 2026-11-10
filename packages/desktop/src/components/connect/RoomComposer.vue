<script setup lang="ts">
import { ref } from "vue";
import InfoRow from "../ui/InfoRow.vue";
import SectionCard from "../ui/SectionCard.vue";
import SegmentedControl from "../ui/SegmentedControl.vue";
import { useRooms } from "../../composables/useRooms";
import { useSettings } from "../../composables/useSettings";
import { useTunnel } from "../../composables/useTunnel";
import { busy } from "../../stores/app";
import type { RoomMode, SegmentedItem } from "../../types";

const modes: SegmentedItem<RoomMode>[] = [
  { key: "create", label: "创建房间" },
  { key: "join", label: "加入房间" },
];
const mode = ref<RoomMode>("create");

const { roomName, joinCode, preview, createRoom, lookupCode, joinResolved } = useRooms();
const { configured } = useSettings();
const { running, pending } = useTunnel();
</script>

<template>
  <SectionCard title="房间">
    <InfoRow label="方式">
      <SegmentedControl v-model="mode" :items="modes" />
    </InfoRow>

    <template v-if="mode === 'create'">
      <InfoRow label="房间名">
        <input
          v-model="roomName"
          class="text-field"
          :disabled="running || busy"
          placeholder="随便填，例如 周五开黑"
          maxlength="24"
          spellcheck="false"
        />
      </InfoRow>
      <InfoRow>
        <div class="row-actions">
          <button
            class="btn-primary small"
            :disabled="!configured || busy || pending"
            @click="createRoom"
          >
            创建并连接
          </button>
        </div>
      </InfoRow>
    </template>

    <template v-else>
      <InfoRow label="房间码">
        <input
          v-model="joinCode"
          class="text-field wide"
          :disabled="running || busy"
          placeholder="ROOM-8K3M 或 LL1. 开头的完整码"
          maxlength="400"
          spellcheck="false"
        />
      </InfoRow>
      <InfoRow>
        <div class="row-actions">
          <button
            class="btn-secondary small"
            :disabled="busy || !joinCode.trim()"
            @click="lookupCode"
          >
            识别房间码
          </button>
          <button
            class="btn-primary small"
            :disabled="!configured || busy || pending || !preview"
            @click="joinResolved"
          >
            加入并连接
          </button>
        </div>
      </InfoRow>
      <InfoRow>
        <span class="row-hint">
          短码会经解析服务查房间配置；解析不通时让对方发「完整房间码」，长码不依赖服务器。
        </span>
      </InfoRow>
    </template>
  </SectionCard>
</template>
