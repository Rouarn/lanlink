import type { TunnelStatus, UiState } from "../types";

/** 把后端状态机映射为界面状态（提权中视为独立的「等待授权」） */
export function uiState(status: TunnelStatus | null): UiState {
  if (!status) return "idle";
  if (status.elevating) return "elevating";
  return status.state;
}

/** 这些状态下正在等待连接结果，应禁用操作按钮 */
export function isPendingState(state: UiState): boolean {
  return state === "connecting" || state === "reconnecting" || state === "elevating";
}
