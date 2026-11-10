import type { LogFilter, RoomRole, RoomKind, SegmentedItem, UiState } from "../types";

export const stateLabel: Record<UiState, string> = {
  idle: "未连接",
  connecting: "连接中",
  connected: "已连接",
  reconnecting: "重连中",
  failed: "连接失败",
  elevating: "等待授权",
};

export const roleLabel: Record<RoomRole, string> = {
  host: "你创建的房间",
  member: "已加入",
};

export const kindLabel: Record<RoomKind, string> = {
  short: "短码",
  long: "完整码（自包含）",
};

/** 中继地址允许的协议前缀，与服务端校验保持一致 */
export const RELAY_SCHEMES = ["tcp://", "udp://", "ws://", "wss://", "quic://", "wg://"];

export const logFilters: SegmentedItem<LogFilter>[] = [
  { key: "all", label: "全部" },
  { key: "info", label: "信息" },
  { key: "warn", label: "警告" },
  { key: "error", label: "错误" },
];

/** 对端连接方式文案 */
export function costLabel(cost: string): string {
  if (cost === "p2p") return "直连";
  if (cost === "relay") return "中继";
  return cost || "—";
}
