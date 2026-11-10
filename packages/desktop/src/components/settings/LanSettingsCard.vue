<script setup lang="ts">
import AppIcon from "../ui/AppIcon.vue";
import McPanel from "../room/lan/McPanel.vue";
import UdpPanel from "../room/lan/UdpPanel.vue";
</script>

<template>
  <section class="card lan-card">
    <header class="card-head">
      <AppIcon name="radio" />
      <h2>局域网发现</h2>
    </header>

    <p class="intro">
      两种方式互相独立，用哪个开哪个。都只在连上房间后生效，不会影响系统里其它网络设置。
    </p>

    <McPanel />
    <div class="divider" />
    <UdpPanel />
  </section>
</template>

<style scoped>
.intro {
  margin: 0 0 14px;
  font-size: var(--text-sm);
  line-height: 1.6;
  color: var(--text-3);
}

.divider {
  height: 1px;
  margin: 16px 0;
  background: var(--border);
}
</style>
