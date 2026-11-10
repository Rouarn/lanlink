<script setup lang="ts">
import SectionCard from "../components/ui/SectionCard.vue";
import { useTunnel } from "../composables/useTunnel";
import { costLabel } from "../lib/labels";

const { peers } = useTunnel();
</script>

<template>
  <SectionCard :title="`在线节点（${peers.length}）`">
    <div v-if="!peers.length" class="empty">连接后可查看同一房间的在线节点</div>
    <div v-for="peer in peers" :key="peer.hostname + peer.ipv4" class="row">
      <div class="peer-main">
        <span class="peer-name">{{ peer.hostname }}</span>
        <span class="peer-ip mono">{{ peer.ipv4 }}</span>
      </div>
      <span class="tag">{{ costLabel(peer.cost) }}</span>
    </div>
  </SectionCard>
</template>
