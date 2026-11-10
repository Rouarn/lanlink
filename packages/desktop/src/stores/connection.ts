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

let pollTimer: number | undefined;
let attemptId = 0;

const phase = computed<Phase>(() => {
  const current = status.value;
  if (localError.value) return "error";
  if (current?.state === "failed" && !dismissed.value) return "error";
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
  if (next.elevate_error) toast(next.elevate_error);
}

async function refresh(): Promise<void> {
  applyStatus(await ipc.getStatus());
}

async function connect(code: string): Promise<void> {
  const id = ++attemptId;
  localError.value = null;
  dismissed.value = false;
  busy.value = true;
  try {
    const next = await ipc.connect(code);
    if (id === attemptId) applyStatus(next);
  } catch (caught) {
    if (id === attemptId) localError.value = humanizeError(caught);
  } finally {
    if (id === attemptId) busy.value = false;
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
