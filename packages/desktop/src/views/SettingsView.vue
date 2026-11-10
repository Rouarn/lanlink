<script setup lang="ts">
import RelayCard from "../components/settings/RelayCard.vue";
import ResolverCard from "../components/settings/ResolverCard.vue";
import LanSettingsCard from "../components/settings/LanSettingsCard.vue";
import AppearanceCard from "../components/settings/AppearanceCard.vue";
import PermissionCard from "../components/settings/PermissionCard.vue";
import AboutCard from "../components/settings/AboutCard.vue";
</script>

<template>
  <section class="screen wide">
    <header class="settings-head">
      <h1>设置</h1>
      <p>改完就生效。中继和解析服务是全局的，房间页会直接用到。</p>
    </header>

    <div class="grid">
      <RelayCard />
      <ResolverCard />
      <LanSettingsCard class="span-2" />
      <AppearanceCard />
      <PermissionCard />
      <AboutCard />
    </div>
  </section>
</template>

<style scoped>
.settings-head {
  margin-bottom: 18px;
}

.settings-head h1 {
  margin: 0 0 4px;
  font-size: var(--text-title);
  font-weight: 660;
  letter-spacing: -0.012em;
}

.settings-head p {
  margin: 0;
  font-size: var(--text-base);
  color: var(--text-3);
}

.grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 14px;
  align-items: start;
}

.grid :deep(.card) {
  margin-bottom: 0;
}

.span-2 {
  grid-column: span 2;
}

@media (max-width: 760px) {
  .grid {
    grid-template-columns: minmax(0, 1fr);
  }

  .span-2 {
    grid-column: auto;
  }
}
</style>
