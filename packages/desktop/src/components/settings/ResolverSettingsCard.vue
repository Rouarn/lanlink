<script setup lang="ts">
import InfoRow from "../ui/InfoRow.vue";
import SectionCard from "../ui/SectionCard.vue";
import { useSettings } from "../../composables/useSettings";
import { useTunnel } from "../../composables/useTunnel";
import { info } from "../../stores/app";

const {
  resolverUrl,
  resolverIsDefault,
  relayDirty,
  saveSettings,
  restoreDefaultResolver,
} = useSettings();
const { running } = useTunnel();
</script>

<template>
  <SectionCard title="房间码解析服务">
    <InfoRow label="服务地址">
      <span class="row-value inline">
        <input
          v-model="resolverUrl"
          class="text-field wide"
          spellcheck="false"
          :placeholder="info?.default_resolver_base_url"
          @input="relayDirty = true"
        />
        <button
          class="chip"
          :disabled="resolverIsDefault || !info?.default_resolver_base_url"
          @click="restoreDefaultResolver"
        >
          恢复默认
        </button>
      </span>
    </InfoRow>
    <InfoRow>
      <span class="row-hint">
        默认使用内置地址，粘贴「ROOM-XXXX」短码即可跨网络解析出完整房间配置；也可以改成自己的服务。
        填成空会恢复默认值。长码（「LL1.」开头）自带配置，不依赖本服务。
      </span>
    </InfoRow>
    <InfoRow>
      <div class="row-actions">
        <span v-if="relayDirty && running" class="footnote warn-text">
          配置已修改，需要重新连接后生效。
        </span>
        <button class="btn-secondary small" @click="saveSettings">保存配置</button>
      </div>
    </InfoRow>
  </SectionCard>
</template>
