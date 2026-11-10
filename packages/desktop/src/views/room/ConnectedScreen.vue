<script setup lang="ts">
import { computed, ref } from "vue";
import { useApp } from "../../stores/app";
import { useConnection } from "../../stores/connection";
import { useRooms } from "../../stores/rooms";
import { useCopyFeedback } from "../../composables/useClipboard";
import { roleLabel, shortCode } from "../../lib/labels";
import AddressCard from "../../components/room/AddressCard.vue";
import OpenGamesCard from "../../components/room/OpenGamesCard.vue";
import LanAssistant from "../../components/room/LanAssistant.vue";
import PeerList from "../../components/room/PeerList.vue";
import LongCodeModal from "../../components/room/LongCodeModal.vue";
import WhyIpModal from "../../components/room/WhyIpModal.vue";
import AppIcon from "../../components/ui/AppIcon.vue";

const { openDrawer, toast } = useApp();
const { status, disconnect, error } = useConnection();
const { activeRoom, isHost, codeWarning, longCode, longError, exportLong, reset } = useRooms();
const { copied, copy } = useCopyFeedback();

const longOpen = ref(false);
const whyOpen = ref(false);
const leaving = ref(false);

const peers = computed(() => status.value?.peers ?? []);
const directCount = computed(() => status.value?.direct_peer_count ?? 0);
const online = computed(() => peers.value.length + 1);

async function copyCode(): Promise<void> {
  const code = activeRoom.value?.code;
  if (!code) return;
  await copy(code);
  toast("房间码已复制");
}

async function openLong(): Promise<void> {
  longOpen.value = true;
  await exportLong();
}

async function leave(): Promise<void> {
  leaving.value = true;
  await disconnect();
  leaving.value = false;
  if (!error.value) reset();
}
</script>

<template>
  <section class="screen">
    <header class="room-head">
      <div class="room-head-main">
        <div class="conn-line">
          <span class="dot ok" />
          已连接
          <span class="conn-sep" />
          {{ online }} 人在房间
          <template v-if="directCount">
            <span class="conn-sep" />
            {{ directCount }} 个直连
          </template>
        </div>

        <h1>{{ activeRoom?.name || "当前房间" }}</h1>

        <div class="room-meta">
          <button type="button" class="code-chip mono" @click="copyCode">
            {{ shortCode(activeRoom?.code || "", 28) }}
            <AppIcon :name="copied ? 'check' : 'copy'" :size="13" />
          </button>
          <span class="chip">{{ roleLabel[activeRoom?.role || "member"] }}</span>
          <button v-if="isHost" type="button" class="link small" @click="openLong">
            完整码
          </button>
        </div>

        <p v-if="codeWarning" class="code-warning">
          <AppIcon name="alert" :size="13" />
          {{ codeWarning }}
        </p>
      </div>

      <button type="button" class="btn btn-danger-quiet" :disabled="leaving" @click="leave">
        {{ leaving ? "正在断开…" : "断开" }}
      </button>
    </header>

    <AddressCard :ip="status?.ipv4 ?? null" @why="whyOpen = true" />

    <OpenGamesCard />

    <LanAssistant />

    <PeerList
      :peers="peers"
      :self-name="status?.hostname ?? null"
      :self-ip="status?.ipv4 ?? null"
      :direct-count="directCount"
    />

    <footer class="room-foot">
      <button type="button" class="link subtle" @click="openDrawer">
        <AppIcon name="activity" :size="13" />
        查看活动日志
      </button>
      <span class="muted">连上后可以关掉这个窗口，网络会保持。</span>
    </footer>

    <LongCodeModal v-model="longOpen" :code="longCode" :error="longError" />
    <WhyIpModal v-model="whyOpen" />
  </section>
</template>

<style scoped>
.room-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 16px;
}

.conn-line {
  display: flex;
  align-items: center;
  gap: 7px;
  margin-bottom: 4px;
  font-size: var(--text-sm);
  font-weight: 600;
  color: var(--ok);
}

.conn-sep {
  width: 1px;
  height: 10px;
  background: var(--border-strong);
}

h1 {
  margin: 0 0 7px;
  font-size: var(--text-title);
  font-weight: 660;
  letter-spacing: -0.012em;
}

.room-meta {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 7px;
}

.code-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 24px;
  padding: 0 8px;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--surface);
  font-size: var(--text-xs);
  color: var(--text-2);
}

.code-chip:hover {
  border-color: var(--border-strong);
  color: var(--text);
}

.code-chip .icon {
  color: var(--text-3);
}

.code-warning {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 8px 0 0;
  font-size: var(--text-sm);
  color: var(--warn);
}

.room-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-top: 6px;
  padding: 0 4px;
  font-size: var(--text-xs);
}
</style>
