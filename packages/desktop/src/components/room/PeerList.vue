<script setup lang="ts">
import { ref } from "vue";
import { useApp } from "../../stores/app";
import { copyPlainText } from "../../lib/clipboard";
import { costLabel, isDirect } from "../../lib/labels";
import type { PeerInfo } from "../../types";
import AppIcon from "../ui/AppIcon.vue";

defineProps<{
  peers: PeerInfo[];
  selfName: string | null;
  selfIp: string | null;
  directCount: number;
}>();

const { toast } = useApp();
const copiedIp = ref("");

/** 对端没回报主机名时（Mac 端目前如此）的占位，避免直接拿 IP 当名字 */
const UNNAMED = "未命名设备";

function displayName(name: string | null | undefined, fallback = UNNAMED): string {
  return name?.trim() || fallback;
}

async function copyIp(ip: string): Promise<void> {
  if (!ip) return;
  await copyPlainText(ip);
  copiedIp.value = ip;
  toast("IP 已复制");
  window.setTimeout(() => {
    if (copiedIp.value === ip) copiedIp.value = "";
  }, 1600);
}
</script>

<template>
  <section class="card peers">
    <header class="card-head">
      <AppIcon name="users" />
      <h2>同一房间</h2>
      <span class="chip">{{ peers.length + 1 }} 人</span>
      <span v-if="directCount" class="chip chip-ok">{{ directCount }} 个直连</span>
    </header>

    <ul class="peer-list">
      <li class="peer-row self">
        <span class="avatar">我</span>
        <span class="peer-main">
          <span class="peer-name">{{ displayName(selfName, "这台电脑") }}</span>
          <span class="peer-ip mono">{{ selfIp || "—" }}</span>
        </span>
        <span class="chip">你</span>
      </li>

      <li v-for="peer in peers" :key="peer.hostname + peer.ipv4" class="peer-row">
        <span class="avatar">{{ displayName(peer.hostname).slice(0, 1).toUpperCase() }}</span>
        <span class="peer-main">
          <span class="peer-name" :class="{ unnamed: !peer.hostname?.trim() }">
            {{ displayName(peer.hostname) }}
          </span>
          <span class="peer-ip mono">{{ peer.ipv4 || "—" }}</span>
        </span>
        <span class="chip" :class="isDirect(peer.cost) ? 'chip-ok' : 'chip-relay'">
          {{ costLabel(peer.cost) }}
        </span>
        <button
          type="button"
          class="icon-btn"
          :aria-label="`复制 ${displayName(peer.hostname)} 的 IP`"
          @click="copyIp(peer.ipv4)"
        >
          <AppIcon :name="copiedIp === peer.ipv4 ? 'check' : 'copy'" :size="14" />
        </button>
      </li>
    </ul>

    <p v-if="!peers.length" class="empty">
      还没看到别人。把房间码发出去，朋友进来后会出现在这里。
    </p>
  </section>
</template>

<style scoped>
.card-head {
  margin-bottom: 10px;
}

.card-head h2 {
  margin-right: auto;
}

.peer-list {
  margin: 0;
  padding: 0;
  list-style: none;
}

.peer-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 0;
}

.peer-row + .peer-row {
  border-top: 1px solid var(--border);
}

.peer-row.self .peer-name {
  color: var(--text-2);
}

.avatar {
  display: grid;
  place-items: center;
  width: 28px;
  height: 28px;
  flex: none;
  border-radius: 50%;
  background: var(--surface-2);
  font-size: var(--text-sm);
  font-weight: 650;
  color: var(--text-2);
}

.self .avatar {
  background: var(--accent-soft);
  color: var(--accent);
}

.peer-main {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
  margin-right: auto;
}

.peer-name {
  font-size: var(--text-base);
  font-weight: 600;
}

.peer-name.unnamed {
  color: var(--text-2);
}

.peer-ip {
  font-size: var(--text-xs);
  color: var(--text-3);
}
</style>
