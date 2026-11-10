import { computed, ref } from "vue";
import { ipc } from "../lib/ipc";
import { isPendingState, uiState } from "../lib/status";
import { activeTab, busy, message } from "../stores/app";
import type { TunnelStatus } from "../types";
import { useLogs } from "./useLogs";
import { useSettings } from "./useSettings";

const status = ref<TunnelStatus | null>(null);

const state = computed(() => uiState(status.value));
const running = computed(() => Boolean(status.value?.running));
const pending = computed(() => isPendingState(state.value));
const peers = computed(() => status.value?.peers ?? []);

let timer: number | undefined;
let polling = false;

/**
 * 轮询间隔。
 *
 * `get_status` 每次都要起 easytier-cli 子进程，Windows 上进程创建很贵（还要过 Defender），
 * 1.5 秒一刷会让整机发闷。所以：稳态放慢到 3 秒；连接中/重连中保持 1 秒，状态切换仍然跟手；
 * 窗口在后台时降到 10 秒，回到前台立刻补刷一次。
 */
const POLL_STABLE = 3000;
const POLL_PENDING = 1000;
const POLL_HIDDEN = 10000;

function pollDelay(): number {
  if (document.hidden) return POLL_HIDDEN;
  return pending.value ? POLL_PENDING : POLL_STABLE;
}

async function refresh(): Promise<void> {
  const logs = useLogs();
  try {
    status.value = await ipc.getStatus();
    // 提权是在后台线程里进行的，失败原因由状态带回来
    if (status.value?.elevate_error) {
      message.value = status.value.elevate_error;
    }
    if (activeTab.value === "logs") {
      await logs.refreshLogs();
    }
  } catch (err) {
    message.value = String(err);
  }
}

async function tick(): Promise<void> {
  timer = undefined;
  if (!polling) return;
  await refresh();
  if (!polling) return;
  timer = window.setTimeout(() => void tick(), pollDelay());
}

/** 从后台切回前台：立刻补一次，避免看到过期状态 */
function onVisibilityChange(): void {
  if (document.hidden || !polling) return;
  if (timer) {
    window.clearTimeout(timer);
    timer = undefined;
  }
  void tick();
}

async function connect(code: string): Promise<void> {
  status.value = await ipc.connect(code);
  if (status.value?.elevating) {
    message.value = "已请求管理员授权，授权后将在新窗口自动连接。";
  }
}

async function disconnect(): Promise<void> {
  busy.value = true;
  message.value = "";
  try {
    status.value = await ipc.disconnect();
    useSettings().markClean();
  } catch (err) {
    message.value = String(err);
  } finally {
    busy.value = false;
  }
}

/** 启动自适应轮询（App 挂载前已经手动刷过一次，这里不再重复请求） */
function startPolling(): void {
  if (polling) return;
  polling = true;
  document.addEventListener("visibilitychange", onVisibilityChange);
  timer = window.setTimeout(() => void tick(), pollDelay());
}

function stopPolling(): void {
  polling = false;
  document.removeEventListener("visibilitychange", onVisibilityChange);
  if (timer) {
    window.clearTimeout(timer);
    timer = undefined;
  }
}

export function useTunnel() {
  return {
    status,
    state,
    running,
    pending,
    peers,
    refresh,
    connect,
    disconnect,
    startPolling,
    stopPolling,
  };
}
