<script setup lang="ts">
import { ref } from "vue";
import { useApp } from "../../../stores/app";
import { useConnection } from "../../../stores/connection";
import { useSettings } from "../../../stores/settings";
import AppIcon from "../../ui/AppIcon.vue";

const { toast } = useApp();
const { applyStatus } = useConnection();
const { manualGames, lanBusy, lanError, setManualGames, validateGamePort } = useSettings();

const name = ref("");
const port = ref("");
const portError = ref("");

async function add(): Promise<void> {
  portError.value = validateGamePort(port.value);
  if (portError.value) return;

  const next = [
    ...manualGames.value,
    { name: name.value.trim() || "局域网游戏", port: Number(port.value) },
  ];
  const nextStatus = await setManualGames(next);
  if (nextStatus) applyStatus(nextStatus);

  toast(`已登记端口 ${port.value}`);
  name.value = "";
  port.value = "";
}

async function remove(target: number): Promise<void> {
  const nextStatus = await setManualGames(
    manualGames.value.filter((game) => game.port !== target),
  );
  if (nextStatus) applyStatus(nextStatus);
  toast("已移除登记的端口");
}
</script>

<template>
  <div class="manual">
    <div class="manual-head">
      <span class="manual-title">手动登记</span>
      <span class="manual-hint">不走自动发现的游戏，填端口就能让房间里的人看到地址</span>
    </div>

    <div class="manual-add">
      <input
        v-model="name"
        class="input input-sm"
        maxlength="32"
        placeholder="游戏名，可留空"
      />
      <input
        v-model="port"
        class="input input-sm mono port"
        maxlength="5"
        inputmode="numeric"
        placeholder="端口"
        @keyup.enter="add"
      />
      <button
        type="button"
        class="btn btn-secondary btn-sm"
        :disabled="lanBusy || !port.trim()"
        @click="add"
      >
        <AppIcon name="plus" :size="14" />
        登记
      </button>
    </div>

    <p v-if="portError" class="field-hint error">{{ portError }}</p>
    <p v-else-if="lanError" class="field-hint error">{{ lanError }}</p>

    <div v-if="manualGames.length" class="manual-list">
      <span v-for="game in manualGames" :key="game.port" class="chip mono manual-chip">
        {{ game.name }} · {{ game.port }}
        <button type="button" :aria-label="`移除 ${game.name}`" @click="remove(game.port)">
          <AppIcon name="x" :size="11" />
        </button>
      </span>
    </div>
  </div>
</template>

<style scoped>
.manual {
  margin-top: var(--space-5);
  padding-top: var(--space-5);
  border-top: 1px solid var(--border);
}

.manual-head {
  display: flex;
  align-items: baseline;
  flex-wrap: wrap;
  gap: 8px;
}

.manual-title {
  font-size: var(--text-base);
  font-weight: 650;
}

.manual-hint {
  font-size: var(--text-xs);
  color: var(--text-3);
}

.manual-add {
  display: flex;
  gap: 6px;
  margin-top: 8px;
}

.manual-add .input {
  flex: 1;
  min-width: 0;
}

.manual-add .port {
  flex: 0 0 76px;
  width: 76px;
}

.manual-list {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
  margin-top: 8px;
}

.manual-chip {
  height: 22px;
  padding: 0 5px 0 7px;
}

.manual-chip button {
  display: grid;
  place-items: center;
  width: 15px;
  height: 15px;
  border-radius: 3px;
  color: var(--text-3);
}

.manual-chip button:hover {
  background: var(--hover);
  color: var(--text);
}
</style>
