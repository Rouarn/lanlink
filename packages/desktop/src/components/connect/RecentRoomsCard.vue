<script setup lang="ts">
import SectionCard from "../ui/SectionCard.vue";
import { useRooms } from "../../composables/useRooms";
import { useSettings } from "../../composables/useSettings";
import { useTunnel } from "../../composables/useTunnel";
import { roleLabel } from "../../lib/labels";
import { busy } from "../../stores/app";
import type { Room } from "../../types";

defineProps<{ rooms: Room[] }>();

const { reconnect, removeRoom, clearRooms } = useRooms();
const { configured } = useSettings();
const { pending } = useTunnel();
</script>

<template>
  <SectionCard title="最近房间">
    <template #action>
      <button class="chip" @click="clearRooms">清空</button>
    </template>
    <div v-for="item in rooms" :key="item.code" class="row">
      <div class="peer-main">
        <span class="peer-name">{{ item.name }}</span>
        <span class="peer-ip mono">{{ item.code }} · {{ roleLabel[item.role] }}</span>
      </div>
      <div class="row-actions">
        <button class="btn-mini" @click="removeRoom(item.code)">删除</button>
        <button
          class="btn-mini"
          :disabled="!configured || busy || pending"
          @click="reconnect(item)"
        >
          连接
        </button>
      </div>
    </div>
  </SectionCard>
</template>
