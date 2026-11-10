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

/** 用户手动登记的一条「开放游戏」 */
export interface ManualGame {
  name: string;
  port: number;
}

/** 房间内某个人开放着的一个游戏端点 */
export interface OpenGame {
  name: string;
  /** 开游戏的人；本机开放时是设备名 */
  hostname: string;
  ipv4: string;
  port: number;
  /** `minecraft` / `manual` / 空字符串 */
  source: string;
  /** 是否本机开放 */
  mine: boolean;
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
  /** 《我的世界》局域网发现中继开关（默认开启） */
  lan_discovery: boolean;
  /** 通用 UDP 广播转发端口表（CS 1.6 / 魔兽 / 帝国时代等），空数组表示关闭 */
  broadcast_ports: number[];
  /** 「开放游戏」手动登记表：只用于展示与复制地址 */
  manual_games: ManualGame[];
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
  /** 设置里的「局域网发现」开关 */
  lan_enabled: boolean;
  /** 局域网发现中继是否在运行 */
  lan_relay: boolean;
  /** 中继启动/运行失败的原因 */
  lan_error: string | null;
  /** 房间内「开放游戏」列表（本机 + 对端） */
  open_games: OpenGame[];
  /** 开放游戏宣告通道是否在运行 */
  games_relay: boolean;
  /** 宣告通道启动/运行失败的原因 */
  games_error: string | null;
}

export interface LogLine {
  level: string;
  time: string;
  message: string;
}

/** 界面状态：在后端状态机之上叠加「等待授权」这一本地状态 */
export type UiState = CoreState | "elevating";

export type LogFilter = "all" | "info" | "warn" | "error";

/** 分段控件的一项 */
export interface SegmentedItem<T extends string> {
  key: T;
  label: string;
}

/** 统一的界面状态机：所有「现在是什么状态」都由它给出，组件不再自己拼。 */
export type Phase =
  | "unconfigured"
  | "idle"
  | "connecting"
  | "connected"
  | "disconnected"
  | "error";

/** 连接中的子状态：等解析 / 等授权 / 建网络 */
export type ConnectStage = "resolving" | "authorizing" | "network" | null;

/** 面向用户的错误：标题与下一步用中文说清楚，技术细节单独折叠。 */
export interface UiError {
  title: string;
  detail: string;
  technical: string;
}

/** 导航目的地：两个，不再有第四个 Tab */
export type ViewKey = "room" | "settings";

/** 房间页内部的两种入口 */
export type RoomScreen = "create" | "join";

/** 平台标识：只用于样式隔离 */
export type Platform = "mac" | "win";

/** 状态胶囊的语气 */
export type Tone = "idle" | "ok" | "warn" | "danger";
