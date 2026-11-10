import type { TunnelStatus } from "../types";

export type LanIssue = { level: "warn" | "error"; message: string };

/**
 * lan_error 并不总是失败。
 * 发现服务仍在运行时（lan_relay 为真），它只是某个子项的降级原因，
 * 例如 UDP 4448 绑不上、个别广播端口不可用 —— 此时应按警告展示。
 * 只有 lan_relay 为假时，才说明发现服务本身没起来，才是失败。
 */
export function lanIssue(status: TunnelStatus | null | undefined): LanIssue | null {
  const message = status?.lan_error?.trim();
  if (!message) return null;
  return status?.lan_relay ? { level: "warn", message } : { level: "error", message };
}
