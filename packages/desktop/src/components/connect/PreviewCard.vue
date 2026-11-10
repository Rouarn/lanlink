<script setup lang="ts">
import InfoRow from "../ui/InfoRow.vue";
import SectionCard from "../ui/SectionCard.vue";
import { kindLabel } from "../../lib/labels";
import type { Room } from "../../types";

defineProps<{ preview: Room }>();
</script>

<template>
  <SectionCard title="解析结果">
    <InfoRow label="房间名">
      <span class="row-value">{{ preview.name }}</span>
    </InfoRow>
    <InfoRow label="码类型">
      <span class="row-value inline">
        {{ kindLabel[preview.kind] }}
        <span v-if="preview.kind === 'short'" class="footnote">
          {{ preview.shared ? "来自解析服务" : "本地码，仅本机可解析" }}
        </span>
      </span>
    </InfoRow>
    <InfoRow label="网络名">
      <span class="row-value mono">{{ preview.network }}</span>
    </InfoRow>
    <InfoRow label="中继服务器" column>
      <span class="row-value mono small multiline">{{ preview.relays.join("\n") || "—" }}</span>
    </InfoRow>
  </SectionCard>
</template>
