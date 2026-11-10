/** 与 Rust 侧 IPC 契约一一对应的类型定义 */

export interface AppInfo {
  elevated: boolean;
  core_path: string;
  cli_path: string;
  rpc_portal: string;
  platform: string;
  /** 内置解析服务地址（唯一出处是 Rust 侧常量，界面「恢复默认」用） */
  default_resolver_base_url: string;
}

export interface PeerInfo {
  hostname: string;
  ipv4: string;
  cost: string;
  proto: string;
}

export type CoreState = "idle" | "connecting" | "connected" | "reconnecting" | "failed";

export type RoomRole = "host" | "member";

/** `short` 本地短码（仅本机可解析）/ `long` 自包含完整码 */
export type RoomKind = "short" | "long";

/** 短码注册到解析服务的结果：已注册 / 尝试过但失败 / 没配解析服务、未尝试 */
export type RoomRegistration = "ok" | "failed" | "skipped";

export interface Room {
  code: string;
  name: string;
  network: string;
  secret: string;
  role: RoomRole;
  relays: string[];
  kind: RoomKind;
  /** 短码是否已注册到解析服务（可跨机器解析） */
  shared: boolean;
  /** 注册结果：界面只在 `failed` 时提示「没有注册成功」 */
  registration: RoomRegistration;
  created_at: number;
  last_used: number;
}

/** 连接页「当前房间」展示用的房间摘要 */
export interface ActiveRoom {
  code: string;
  name: string;
  role: RoomRole;
  kind: RoomKind;
  registration: RoomRegistration;
}

export interface Settings {
  relays: string[];
  resolver_base_url: string | null;
  /** Windows：是否已开启计划任务静默提权 */
  silent_elevate: boolean;
  /** Windows：首次授权说明是否已经问过一次 */
  silent_prompted: boolean;
}

/** 提权方式（Windows 计划任务静默提权） */
export interface ElevationInfo {
  platform: string;
  elevated: boolean;
  silent: boolean;
  task_present: boolean;
}

export interface TunnelStatus {
  state: CoreState;
  running: boolean;
  elevated: boolean;
  elevating: boolean;
  ipv4: string | null;
  hostname: string | null;
  peer_count: number;
  direct_peer_count: number;
  version: string | null;
  core_pid: number | null;
  error: string | null;
  hint: string | null;
  room: string | null;
  room_name: string | null;
  role: RoomRole | null;
  network: string | null;
  /** 提权失败的提示，后端只回传一次 */
  elevate_error: string | null;
  log_tail: string[];
  peers: PeerInfo[];
}

export interface LogLine {
  level: string;
  time: string;
  message: string;
}

/** 界面状态：在后端状态机之上叠加「等待授权」这一本地状态 */
export type UiState = CoreState | "elevating";

/** 创建房间 / 加入房间 */
export type RoomMode = "create" | "join";

export type LogFilter = "all" | "info" | "warn" | "error";

/** 分段控件的一项 */
export interface SegmentedItem<T extends string> {
  key: T;
  label: string;
}
