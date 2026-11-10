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

/** 启动 1.5s 轮询，刷新连接状态 */
function startPolling(): void {
  timer = window.setInterval(() => void refresh(), 1500);
}

function stopPolling(): void {
  if (timer) {
    window.clearInterval(timer);
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
