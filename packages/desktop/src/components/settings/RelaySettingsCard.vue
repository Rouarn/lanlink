<script setup lang="ts">
import InfoRow from "../ui/InfoRow.vue";
import SectionCard from "../ui/SectionCard.vue";
import { useSettings } from "../../composables/useSettings";

const { relayText, relayError, settingsSaved, settingsError, relayDirty, saveSettings } =
  useSettings();
</script>

<template>
  <SectionCard title="中继服务器">
    <div class="row column">
      <textarea
        v-model="relayText"
        class="text-area"
        rows="5"
        spellcheck="false"
        placeholder="tcp://你的中继地址:11010"
        @input="relayDirty = true"
      ></textarea>
      <span class="row-hint">
        每行一条，支持 tcp:// udp:// ws:// wss://；留空表示未配置，未配置时无法创建或加入房间。
      </span>
      <span v-if="relayError" class="footnote error">{{ relayError }}</span>
    </div>
    <InfoRow>
      <div class="row-actions">
        <span v-if="settingsSaved" class="footnote inline-ok">已保存</span>
        <button class="btn-primary small" :disabled="Boolean(relayError)" @click="saveSettings">
          保存配置
        </button>
      </div>
    </InfoRow>
    <InfoRow v-if="settingsError">
      <span class="footnote error">{{ settingsError }}</span>
    </InfoRow>
  </SectionCard>
</template>
