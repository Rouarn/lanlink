<script setup lang="ts">
import InfoRow from "../ui/InfoRow.vue";
import SectionCard from "../ui/SectionCard.vue";
import { useSettings } from "../../composables/useSettings";
import { useTunnel } from "../../composables/useTunnel";
import { info } from "../../stores/app";

const { relayLabel } = useSettings();
const { status } = useTunnel();
</script>

<template>
  <SectionCard title="运行环境">
    <InfoRow label="core 版本">
      <span class="row-value mono">{{ status?.version ?? "—" }}</span>
    </InfoRow>
    <InfoRow label="运行权限">
      <span class="row-value">{{ info?.elevated ? "管理员（root）" : "当前用户" }}</span>
    </InfoRow>
    <InfoRow label="进程 PID">
      <span class="row-value mono">{{ status?.core_pid ?? "—" }}</span>
    </InfoRow>
    <InfoRow label="当前中继">
      <span class="row-value mono small path">{{ relayLabel || "未配置" }}</span>
    </InfoRow>
  </SectionCard>

  <SectionCard title="路径">
    <InfoRow label="easytier-core">
      <span class="row-value mono small path">{{ info?.core_path ?? "—" }}</span>
    </InfoRow>
    <InfoRow label="easytier-cli">
      <span class="row-value mono small path">{{ info?.cli_path ?? "—" }}</span>
    </InfoRow>
    <InfoRow label="本地 RPC">
      <span class="row-value mono small">{{ info?.rpc_portal ?? "—" }}</span>
    </InfoRow>
    <InfoRow label="core 日志">
      <span class="row-value mono small path">/tmp/lanlink/core.log</span>
    </InfoRow>
  </SectionCard>

  <SectionCard title="关于">
    <InfoRow label="版本">
      <span class="row-value">0.1.0</span>
    </InfoRow>
  </SectionCard>
</template>
