import type { AppInfo, TunnelStatus } from "../types";
import { lanIssue } from "./lan";

function lanText(status: TunnelStatus | null): string {
  if (!status?.lan_enabled) return "已关闭";
  const issue = lanIssue(status);
  if (!status.lan_relay) return issue ? `未运行（${issue.message}）` : "已开启未运行";
  return issue ? `运行中（警告：${issue.message}）` : "运行中";
}

/** 复制诊断信息：出问题时直接发给对方，不用自己拼字段 */
export function buildDiagnostics(
  info: AppInfo | null,
  status: TunnelStatus | null,
  version = "",
): string {
  if (!info) return "LanLink 诊断信息尚未加载";
  return [
    `LanLink ${version || "—"} · ${info.platform}`,
    `core: ${status?.version ?? "—"}`,
    `权限: ${info.elevated ? "管理员" : "当前用户"}`,
    `状态: ${status?.state ?? "idle"}`,
    `房间: ${status?.room_name ?? "—"} (${status?.room ?? "—"})`,
    `虚拟 IP: ${status?.ipv4 ?? "—"}`,
    `中继: ${status?.room ? "已使用房间中继" : "—"}`,
    `局域网发现: ${lanText(status)}`,
    `core 路径: ${info.core_path}`,
    `cli 路径: ${info.cli_path}`,
  ].join("\n");
}
