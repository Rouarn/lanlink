import type { RoomKind, RoomRegistration, RoomRole } from "../types";

export const roleLabel: Record<RoomRole, string> = {
  host: "房主",
  member: "成员",
};

export const kindLabel: Record<RoomKind, string> = {
  short: "短码",
  long: "完整码",
};

export const registrationLabel: Record<RoomRegistration, string> = {
  ok: "短码已注册",
  failed: "短码未注册",
  skipped: "未走解析服务",
};

/** 对端线路：p2p / direct 视为直连，其余按中继展示 */
export function costLabel(cost: string): string {
  const value = cost.toLowerCase();
  return value.includes("p2p") || value.includes("direct") ? "直连" : "经中继";
}

export function isDirect(cost: string): boolean {
  const value = cost.toLowerCase();
  return value.includes("p2p") || value.includes("direct");
}

/** 房间码过长时中间省略，保留可辨认的头尾 */
export function shortCode(code: string, max = 22): string {
  if (code.length <= max) return code;
  return `${code.slice(0, max - 8)}…${code.slice(-7)}`;
}
