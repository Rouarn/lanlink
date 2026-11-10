import { invoke } from "@tauri-apps/api/core";
import type {
  AppInfo,
  ElevationInfo,
  LogFilter,
  LogLine,
  Room,
  Settings,
  TunnelStatus,
} from "../types";

/**
 * 与 Rust 后端交互的唯一入口。
 * 组件与组合式函数只调用这里的具名方法，不直接出现 invoke 命令字符串。
 */
export const ipc = {
  appInfo: (): Promise<AppInfo> => invoke<AppInfo>("app_info"),

  getStatus: (): Promise<TunnelStatus> => invoke<TunnelStatus>("get_status"),

  connect: (code: string): Promise<TunnelStatus> => invoke<TunnelStatus>("connect", { code }),

  disconnect: (): Promise<TunnelStatus> => invoke<TunnelStatus>("disconnect"),

  getSettings: (): Promise<Settings> => invoke<Settings>("get_settings"),

  saveSettings: (input: {
    relays: string;
    resolverBaseUrl: string;
  }): Promise<Settings> => invoke<Settings>("save_settings", input),

  elevationInfo: (): Promise<ElevationInfo> => invoke<ElevationInfo>("elevation_info"),

  setSilentElevate: (enabled: boolean): Promise<ElevationInfo> =>
    invoke<ElevationInfo>("set_silent_elevate", { enabled }),

  /** 开关《我的世界》局域网发现中继，立即生效 */
  setLanDiscovery: (enabled: boolean): Promise<TunnelStatus> =>
    invoke<TunnelStatus>("set_lan_discovery", { enabled }),

  /** 通用 UDP 广播转发的端口表；空数组表示关闭，连接中会立刻生效 */
  setBroadcastPorts: (ports: number[]): Promise<TunnelStatus> =>
    invoke<TunnelStatus>("set_broadcast_ports", { ports }),

  createRoom: (name: string): Promise<Room> => invoke<Room>("create_room", { name }),

  resolveCode: (code: string): Promise<Room> => invoke<Room>("resolve_code", { code }),

  exportRoomCode: (code: string): Promise<string> =>
    invoke<string>("export_room_code", { code }),

  recentRooms: (): Promise<Room[]> => invoke<Room[]>("recent_rooms"),

  forgetRoom: (code: string): Promise<void> => invoke("forget_room", { code }),

  clearRooms: (): Promise<void> => invoke("clear_rooms"),

  readLogs: (level: LogFilter, limit: number): Promise<LogLine[]> =>
    invoke<LogLine[]>("read_logs", { level, limit }),

  clearLogs: (): Promise<void> => invoke("clear_logs"),
};
