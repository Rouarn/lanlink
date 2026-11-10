<script setup lang="ts">
import InfoRow from "../ui/InfoRow.vue";
import SectionCard from "../ui/SectionCard.vue";
import StatusDot from "../ui/StatusDot.vue";
import { useRooms } from "../../composables/useRooms";
import { useTunnel } from "../../composables/useTunnel";
import { roleLabel, stateLabel } from "../../lib/labels";
import { busy } from "../../stores/app";
import type { ActiveRoom } from "../../types";

defineProps<{
  activeRoom: ActiveRoom;
  /** 是否拿到了完整房间记录（决定能否导出完整码） */
  exportable: boolean;
}>();

defineEmits<{ disconnect: [] }>();

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
        <button v-if="activeRoom.kind === 'short'" class="chip" @click="copyRoomCode(activeRoom.code)">
          {{ copied ? "已复制" : "复制" }}
        </button>
      </span>
    </InfoRow>
    <!-- 注册状态只对房主成立；且只在确实有注册结果时展示，
         没有数据（已断开/刚启动/加入方）一律不显示，不能拿「失败」当兜底。 -->
    <InfoRow
      v-if="
        activeRoom.role === 'host' &&
        activeRoom.kind === 'short' &&
        activeRoom.registration === 'ok'
      "
    >
      <span class="row-hint">已注册到解析服务，朋友可用这个短码加入。</span>
    </InfoRow>
    <template
      v-if="
        activeRoom.role === 'host' &&
        activeRoom.kind === 'short' &&
        activeRoom.registration === 'failed'
      "
    >
      <InfoRow>
        <span class="row-hint">未注册到解析服务，暂时只有本机可解析；发给朋友请用完整房间码。</span>
      </InfoRow>
      <InfoRow>
        <span class="row-hint warn-text">
          短码没有注册成功（解析服务不可达或地址有误），暂时只有本机可解析。
        </span>
      </InfoRow>
    </template>
    <!-- 分享完整房间码是房主的事，加入方不展示 -->
    <InfoRow v-if="activeRoom.role === 'host'" label="完整房间码" column>
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
    <!-- 断开入口放在卡片里，连上之后不用再往下找 -->
    <InfoRow v-if="status?.running">
      <div class="row-actions">
        <button class="btn-secondary" :disabled="busy" @click="$emit('disconnect')">
          {{ busy ? "处理中…" : "断开连接" }}
        </button>
      </div>
    </InfoRow>
  </SectionCard>
</template>
