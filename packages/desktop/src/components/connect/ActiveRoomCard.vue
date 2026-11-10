<script setup lang="ts">
import InfoRow from "../ui/InfoRow.vue";
import SectionCard from "../ui/SectionCard.vue";
import StatusDot from "../ui/StatusDot.vue";
import { useRooms } from "../../composables/useRooms";
import { useTunnel } from "../../composables/useTunnel";
import { roleLabel, stateLabel } from "../../lib/labels";
import type { ActiveRoom } from "../../types";

defineProps<{
  activeRoom: ActiveRoom;
  /** 短码是否已注册到解析服务 */
  shared: boolean;
  /** 是否拿到了完整房间记录（决定能否导出完整码） */
  exportable: boolean;
  /** 是否已配置解析服务地址 */
  resolverConfigured: boolean;
}>();

const {
  longCode,
  longError,
  copied,
  longCopied,
  exportLong,
  copyRoomCode,
  copyLongCode,
} = useRooms();
const { status, state } = useTunnel();
</script>

<template>
  <SectionCard title="当前房间">
    <InfoRow label="房间名">
      <span class="row-value">{{ activeRoom.name }}</span>
    </InfoRow>
    <InfoRow label="房间码">
      <span class="row-value inline">
        <span class="mono code-value">
          {{ activeRoom.kind === "long" ? "完整码（自包含）" : activeRoom.code }}
        </span>
        <button
          v-if="activeRoom.kind === 'short'"
          class="chip"
          @click="copyRoomCode(activeRoom.code)"
        >
          {{ copied ? "已复制" : "复制" }}
        </button>
      </span>
    </InfoRow>
    <InfoRow v-if="activeRoom.kind === 'short'">
      <span v-if="shared" class="row-hint">已注册到解析服务，朋友可用这个短码加入。</span>
      <span v-else class="row-hint">未注册到解析服务，暂时只有本机可解析；发给朋友请用完整房间码。</span>
    </InfoRow>
    <InfoRow v-if="activeRoom.kind === 'short' && !shared && resolverConfigured">
      <span class="row-hint warn-text">
        短码没有注册成功（解析服务不可达或地址有误），暂时只有本机可解析。
      </span>
    </InfoRow>
    <InfoRow label="完整房间码" column>
      <div class="row-actions">
        <span v-if="longCode" class="row-hint">长度 {{ longCode.length }} 字符</span>
        <button class="chip" :disabled="!exportable" @click="exportLong">导出完整房间码</button>
        <button v-if="longCode" class="chip" @click="copyLongCode">
          {{ longCopied ? "已复制" : "复制" }}
        </button>
      </div>
      <textarea v-if="longCode" class="text-area" rows="3" readonly :value="longCode"></textarea>
      <span v-if="longCode" class="row-hint">自包含网络名、密钥与中继，可以直接发给朋友。</span>
      <span v-if="longError" class="footnote error">{{ longError }}</span>
    </InfoRow>
    <InfoRow label="身份">
      <span class="row-value">{{ roleLabel[activeRoom.role] }}</span>
    </InfoRow>
    <InfoRow label="状态">
      <span class="row-value inline">
        <StatusDot :state="state" />
        {{ stateLabel[state] }}
      </span>
    </InfoRow>
    <InfoRow label="虚拟 IP">
      <span class="row-value mono">{{ status?.ipv4 ?? "—" }}</span>
    </InfoRow>
    <InfoRow label="对端数量">
      <span class="row-value">{{ status?.peer_count ?? 0 }}</span>
    </InfoRow>
    <InfoRow label="直连数量">
      <span class="row-value">{{ status?.direct_peer_count ?? 0 }}</span>
    </InfoRow>
  </SectionCard>
</template>
