import { computed, ref } from "vue";
import { ipc } from "../lib/ipc";
import { humanizeError } from "../lib/errors";
import { useApp } from "./app";
import { useSettings } from "./settings";
import type { ConnectStage, Phase, Tone, TunnelStatus, UiError } from "../types";

/**
 * 唯一的连接状态机。所有组件只读 phase / stage / pill / error，
 * 不再各自判断 running、state、elevating 的组合。
 */
const { toast } = useApp();
const settings = useSettings();

const status = ref<TunnelStatus | null>(null);
const resolving = ref(false);
const localError = ref<UiError | null>(null);
const dismissed = ref(false);
const everConnected = ref(false);
const busy = ref(false);
/** 有一次连接请求在途：期间画面停在「连接中」，不因为上一轮的失败状态弹回房间页 */
const attempting = ref(false);

let pollTimer: number | undefined;
let attemptId = 0;

const phase = computed<Phase>(() => {
  const current = status.value;
  if (localError.value) return "error";
  if (current?.state === "failed" && !dismissed.value) return "error";
  // 新一轮连接已经发出、结果还没回来：先按「连接中」展示，
  // 否则 status 里上一次的 failed 会把画面弹回房间页，看起来像点了没反应。
  if (attempting.value) return "connecting";
  if (current?.running && current.state === "connected") return "connected";
  if (
    current?.elevating ||
    current?.state === "connecting" ||
    current?.state === "reconnecting"
  ) {
    return "connecting";
  }
  if (settings.configured.value) return everConnected.value ? "disconnected" : "idle";
  return "unconfigured";
});

const stage = computed<ConnectStage>(() => {
  if (resolving.value) return "resolving";
  const current = status.value;
  if (!current) return null;
  if (current.elevating) return "authorizing";
  if (current.state === "connecting" || current.state === "reconnecting") return "network";
  return null;
});

const error = computed<UiError | null>(() => {
  if (localError.value) return localError.value;
  if (status.value?.state === "failed" && !dismissed.value) {
    return humanizeError(status.value.error, status.value.hint);
  }
  return null;
});

const pill = computed<{ label: string; tone: Tone }>(() => {
  switch (phase.value) {
    case "unconfigured":
      return { label: "未配置中继", tone: "idle" };
    case "idle":
      return { label: "已配置 · 未连接", tone: "idle" };
    case "connecting":
      return {
        label:
          stage.value === "resolving"
            ? "正在解析房间码"
            : stage.value === "authorizing"
              ? "等待管理员授权"
              : "正在建立连接",
        tone: "warn",
      };
    case "connected":
      return { label: "已连接", tone: "ok" };
    case "disconnected":
      return { label: "已断开", tone: "idle" };
    case "error":
      return { label: "连接失败", tone: "danger" };
  }
});

function applyStatus(next: TunnelStatus): void {
  status.value = next;
  if (next.running && next.state === "connected") everConnected.value = true;
  // 离开 failed 之后「已忽略」就没有意义了：下次再失败必须重新提示，
  // 不能因为用户忽略过一次旧错误就把后面的失败一直吞掉。
  if (next.state !== "failed") dismissed.value = false;
  if (next.elevate_error) toast(next.elevate_error);
}

async function refresh(): Promise<void> {
  applyStatus(await ipc.getStatus());
}

async function connect(code: string): Promise<void> {
  const id = ++attemptId;
  localError.value = null;
  busy.value = true;
  attempting.value = true;
  try {
    const next = await ipc.connect(code);
    if (id !== attemptId) return;
    // 新一轮的真实结果回来之后才清「已忽略」：请求发出时就清，会把上一次的失败
    // 当成这一次的结果重新弹出来（也就是「点了重试，屏幕上还是旧错误」）。
    dismissed.value = false;
    applyStatus(next);
  } catch (caught) {
    if (id === attemptId) localError.value = humanizeError(caught);
  } finally {
    if (id === attemptId) {
      busy.value = false;
      attempting.value = false;
    }
  }
}

async function disconnect(): Promise<void> {
  attemptId += 1;
  busy.value = true;
  try {
    applyStatus(await ipc.disconnect());
  } catch (caught) {
    localError.value = humanizeError(caught);
  } finally {
    busy.value = false;
  }
}

/** 取消连接：立刻断开并让在途的 connect 结果失效 */
async function cancel(): Promise<void> {
  await disconnect();
}

function dismissError(): void {
  localError.value = null;
  dismissed.value = true;
}

/** 连接之外的失败（建房间、导出完整码等）也走同一张错误页 */
function fail(raw: unknown, hint?: string | null): void {
  dismissed.value = false;
  localError.value = humanizeError(raw, hint);
  status.value = null;
}

async function withResolving<T>(task: () => Promise<T>): Promise<T> {
  resolving.value = true;
  localError.value = null;
  dismissed.value = false;
  try {
    return await task();
  } finally {
    resolving.value = false;
  }
}

function pollDelay(): number {
  if (phase.value === "connecting") return 1000;
  if (phase.value === "connected") return 2000;
  return 4000;
}

function startPolling(): void {
  if (pollTimer) return;
  const tick = async (): Promise<void> => {
    try {
      await refresh();
    } catch {
      // 轮询失败不打扰用户，下一轮继续
    }
    pollTimer = window.setTimeout(tick, pollDelay());
  };
  void tick();
}

function stopPolling(): void {
  window.clearTimeout(pollTimer);
  pollTimer = undefined;
}

export function useConnection() {
  return {
    status,
    phase,
    stage,
    error,
    pill,
    busy,
    resolving,
    everConnected,
    refresh,
    connect,
    disconnect,
    cancel,
    dismissError,
    fail,
    withResolving,
    applyStatus,
    startPolling,
    stopPolling,
  };
}
