import type { AppInfo, TunnelStatus } from "../types";

/** 复制诊断信息：出问题时直接发给对方，不用自己拼字段 */
export function buildDiagnostics(info: AppInfo | null, status: TunnelStatus | null): string {
  if (!info) return "LanLink 诊断信息尚未加载";
  return [
    `LanLink 0.1.0 · ${info.platform}`,
    `core: ${status?.version ?? "—"}`,
    `权限: ${info.elevated ? "管理员" : "当前用户"}`,
    `状态: ${status?.state ?? "idle"}`,
    `房间: ${status?.room_name ?? "—"} (${status?.room ?? "—"})`,
    `虚拟 IP: ${status?.ipv4 ?? "—"}`,
    `中继: ${status?.room ? "已使用房间中继" : "—"}`,
    `局域网发现: ${status?.lan_enabled ? (status?.lan_relay ? "运行中" : "已开启未运行") : "已关闭"}`,
    `core 路径: ${info.core_path}`,
    `cli 路径: ${info.cli_path}`,
  ].join("\n");
}
