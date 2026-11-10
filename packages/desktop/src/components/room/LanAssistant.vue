<script setup lang="ts">
import { computed, ref } from "vue";
import { useConnection } from "../../stores/connection";
import { useSettings } from "../../stores/settings";
import AppIcon from "../ui/AppIcon.vue";
import McPanel from "./lan/McPanel.vue";
import UdpPanel from "./lan/UdpPanel.vue";

type LanTab = "mc" | "udp";

const { phase, status } = useConnection();
const { lanDiscovery, broadcastPorts } = useSettings();

const tab = ref<LanTab>("mc");
const tabs: Array<{ key: LanTab; label: string }> = [
  { key: "mc", label: "我的世界" },
  { key: "udp", label: "通用 UDP" },
];

const anyOn = computed(() => lanDiscovery.value || broadcastPorts.value.length > 0);
const live = computed(
  () =>
    phase.value === "connected" &&
    Boolean(status.value?.lan_relay || broadcastPorts.value.length > 0),
);

const badge = computed(() => {
  if (!anyOn.value) return { label: "已关闭", tone: "idle" };
  if (live.value) return { label: "正在广播", tone: "ok" };
  return { label: "连上后生效", tone: "idle" };
});
</script>

<template>
  <section class="card lan">
    <header class="card-head">
      <AppIcon name="radio" />
      <h2>局域网自动发现</h2>
      <span class="pill" :class="`pill-${badge.tone}`">
        <span class="dot" :class="badge.tone" />
        {{ badge.label }}
      </span>
    </header>

    <p class="lan-intro">
      让同一房间的朋友在游戏的「局域网」列表里直接看到你。两种方式各管一类游戏，互不影响。
    </p>

    <div class="segmented">
      <button
        v-for="item in tabs"
        :key="item.key"
        type="button"
        class="seg-item"
        :class="{ active: tab === item.key }"
        @click="tab = item.key"
      >
        {{ item.label }}
      </button>
    </div>

    <McPanel v-if="tab === 'mc'" />
    <UdpPanel v-else />
  </section>
</template>

<style scoped>
.card-head {
  margin-bottom: 8px;
}

.card-head h2 {
  margin-right: auto;
}

.lan-intro {
  margin: 0 0 12px;
  font-size: var(--text-sm);
  line-height: 1.6;
  color: var(--text-3);
}
</style>
