import type { TunnelStatus } from "../types";

export type LanIssue = { level: "warn" | "error"; message: string };

/**
 * lan_error 只在《我的世界》局域网发现服务本身没起来时才有值
 * （例如组播端口绑不上、找不到可用网卡），此时按错误展示。
 * 服务仍在运行（lan_relay 为真）时不应带这个字段，带上了也按警告展示。
 */
export function lanIssue(status: TunnelStatus | null | undefined): LanIssue | null {
  const message = status?.lan_error?.trim();
  if (!message) return null;
  return status?.lan_relay ? { level: "warn", message } : { level: "error", message };
}
