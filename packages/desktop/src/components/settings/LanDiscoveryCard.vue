<script setup lang="ts">
import InfoRow from "../ui/InfoRow.vue";
import SectionCard from "../ui/SectionCard.vue";
import { useSettings } from "../../composables/useSettings";

const { lanDiscovery, lanPending, lanError, setLanDiscovery } = useSettings();
</script>

<template>
  <SectionCard title="局域网发现">
    <InfoRow label="当前状态">
      <span class="row-value">{{ lanDiscovery ? "已开启" : "已关闭" }}</span>
    </InfoRow>
    <InfoRow>
      <div class="row-actions">
        <button
          class="btn-secondary small"
          :disabled="lanPending"
          @click="setLanDiscovery(!lanDiscovery)"
        >
          {{
            lanPending
              ? "处理中…"
              : lanDiscovery
                ? "关闭局域网发现"
                : "开启局域网发现"
          }}
        </button>
      </div>
    </InfoRow>
    <span class="row-hint">
      开启后，房间内谁在《我的世界》Java 版里「对局域网开放」，其他成员的「多人游戏」列表会自动刷出这个世界；
      从局域网列表点进去才能用离线账号（手动「直接连接」填 IP 会报「无效会话」）。
      只对 MC Java 的局域网公告（224.0.2.60:4445）有效，其它游戏的广播不一定适用。
      只在连接房间期间运行，断开后立即停止并关闭本地转发端口。
    </span>
    <span v-if="lanError" class="footnote error">{{ lanError }}</span>
  </SectionCard>
</template>
