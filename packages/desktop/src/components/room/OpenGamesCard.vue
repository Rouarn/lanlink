<script setup lang="ts">
import { computed, ref } from "vue";
import { useApp } from "../../stores/app";
import { useConnection } from "../../stores/connection";
import { copyPlainText } from "../../lib/clipboard";
import type { OpenGame } from "../../types";
import AppIcon from "../ui/AppIcon.vue";
import ManualGameForm from "./games/ManualGameForm.vue";

const { toast } = useApp();
const { status } = useConnection();

const games = computed(() => status.value?.open_games ?? []);
/// 有对端《我的世界》世界时补一句说明：游戏内的局域网列表不会显示上面的地址，
/// 而是本机物理网卡地址 + 本机转发端口（跨网络时也一样），免得被当成"本地残留"。
const bridged = computed(() =>
  games.value.some((game) => !game.mine && game.source === "minecraft"),
);
const error = computed(() => {
  const message = status.value?.games_error?.trim();
  return message ? `开放游戏同步没起来：${message}` : "";
});

const copied = ref("");

function endpoint(game: OpenGame): string {
  return `${game.ipv4}:${game.port}`;
}

function sourceLabel(source: string): string {
  if (source === "minecraft") return "我的世界";
  if (source === "manual") return "手动登记";
  return "局域网游戏";
}

async function copy(game: OpenGame): Promise<void> {
  const address = endpoint(game);
  await copyPlainText(address);
  copied.value = address;
  toast("游戏地址已复制");
  window.setTimeout(() => {
    if (copied.value === address) copied.value = "";
  }, 1600);
}
</script>

<template>
  <section class="card">
    <header class="card-head">
      <AppIcon name="gamepad" />
      <h2>开放游戏</h2>
      <span v-if="games.length" class="chip">{{ games.length }} 个地址</span>
    </header>

    <p class="games-intro">
      房间里谁开着局域网游戏，会在这里出现。复制地址后，在游戏的「直连 / 添加服务器」里粘贴。
    </p>

    <ul v-if="games.length" class="game-list">
      <li v-for="game in games" :key="endpoint(game)" class="game-row">
        <span class="game-avatar">
          <AppIcon name="gamepad" :size="15" />
        </span>
        <span class="game-main">
          <span class="game-name">{{ game.name }}</span>
          <span class="game-meta">
            {{ game.mine ? "本机" : game.hostname }} · {{ sourceLabel(game.source) }}
          </span>
        </span>
        <button
          type="button"
          class="endpoint mono"
          :aria-label="`复制 ${endpoint(game)}`"
          @click="copy(game)"
        >
          {{ endpoint(game) }}
          <AppIcon :name="copied === endpoint(game) ? 'check' : 'copy'" :size="13" />
        </button>
      </li>
    </ul>

    <p v-else class="empty">
      还没有人开放游戏。开着《我的世界》的「对局域网开放」，或在下面手动登记一个端口。
    </p>

    <p v-if="bridged" class="games-hint">
      《我的世界》「多人游戏」里显示的会是<b>本机局域网地址 + 本机转发端口</b>，端口是
      本机临时分配的，每次开放都可能不同；它和上面的地址是同一个世界，跨网络时也一样，
      不是本地残留。
    </p>

    <p v-if="error" class="field-hint warn">
      <AppIcon name="alert" :size="13" />
      {{ error }}
    </p>

    <ManualGameForm />
  </section>
</template>

<style scoped>
.card-head {
  margin-bottom: 8px;
}

.card-head h2 {
  margin-right: auto;
}

.games-intro {
  margin: 0 0 12px;
  font-size: var(--text-sm);
  line-height: 1.6;
  color: var(--text-3);
}

.game-list {
  margin: 0;
  padding: 0;
  list-style: none;
}

.game-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 0;
}

.game-row + .game-row {
  border-top: 1px solid var(--border);
}

.game-avatar {
  display: grid;
  place-items: center;
  width: 28px;
  height: 28px;
  flex: none;
  border-radius: var(--radius-sm);
  background: var(--surface-2);
  color: var(--text-2);
}

.game-main {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
  margin-right: auto;
}

.game-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--text-base);
  font-weight: 600;
}

.game-meta {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--text-xs);
  color: var(--text-3);
}

.endpoint {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  flex: none;
  height: 26px;
  padding: 0 8px;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--surface);
  font-size: var(--text-xs);
  color: var(--text-2);
}

.endpoint:hover {
  border-color: var(--border-strong);
  color: var(--text);
}

.endpoint .icon {
  color: var(--text-3);
}

.empty {
  margin: 0;
  padding: 14px 0 2px;
  text-align: left;
  font-size: var(--text-sm);
  line-height: 1.7;
  color: var(--text-3);
}

.games-hint {
  margin: 0 0 8px;
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--surface-2);
  font-size: var(--text-xs);
  line-height: 1.7;
  color: var(--text-3);
}

.games-hint b {
  color: var(--text-2);
  font-weight: 600;
}
</style>
