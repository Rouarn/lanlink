<script setup lang="ts">
import ElevationSettingsCard from "../components/settings/ElevationSettingsCard.vue";
import RelaySettingsCard from "../components/settings/RelaySettingsCard.vue";
import ResolverSettingsCard from "../components/settings/ResolverSettingsCard.vue";
import RuntimeInfoCards from "../components/settings/RuntimeInfoCards.vue";
</script>

<template>
  <RelaySettingsCard />
  <ResolverSettingsCard />
  <ElevationSettingsCard />
  <RuntimeInfoCards />
</template>
