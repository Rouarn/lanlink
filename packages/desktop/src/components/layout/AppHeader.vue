<script setup lang="ts">
import { pageTitle } from "../../stores/app";
import { useTunnel } from "../../composables/useTunnel";
import { stateLabel } from "../../lib/labels";
import StatusDot from "../ui/StatusDot.vue";

const { state } = useTunnel();
</script>

<template>
  <header class="content-header" data-tauri-drag-region>
    <h1>{{ pageTitle }}</h1>
    <span class="status-pill">
      <StatusDot :state="state" />
      {{ stateLabel[state] }}
    </span>
  </header>
</template>
