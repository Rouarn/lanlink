import { nextTick, ref } from "vue";
import { ipc } from "../lib/ipc";
import { message } from "../stores/app";
import type { LogFilter, LogLine } from "../types";
import { useCopyFeedback } from "./useClipboard";

const logs = ref<LogLine[]>([]);
const logLevel = ref<LogFilter>("all");
const logBox = ref<HTMLElement | null>(null);
const logCopy = useCopyFeedback();

/** 拉取日志；用户停留在底部时自动跟随滚动 */
async function refreshLogs(): Promise<void> {
  const box = logBox.value;
  const atBottom = box ? box.scrollTop + box.clientHeight >= box.scrollHeight - 28 : true;
  try {
    logs.value = await ipc.readLogs(logLevel.value, 400);
  } catch (err) {
    message.value = String(err);
    return;
  }
  if (atBottom) {
    await nextTick();
    if (logBox.value) logBox.value.scrollTop = logBox.value.scrollHeight;
  }
}

async function copyLogs(): Promise<void> {
  const text = logs.value
    .map((line) => [line.time, line.level, line.message].filter(Boolean).join(" "))
    .join("\n");
  await logCopy.copy(text);
}

/**
 * 清空日志：截断 core.log（真清空，不只是清界面），再把列表清空、滚动位置复位。
 * 清完后不再立刻回读——core 仍在运行时会马上追加新行，立刻回读会让人以为没清掉。
 * 新写入的内容在下次进入日志页或切换筛选时出现。
 */
async function clearLogs(): Promise<void> {
  try {
    await ipc.clearLogs();
  } catch (err) {
    message.value = String(err);
    return;
  }
  logs.value = [];
  if (logBox.value) logBox.value.scrollTop = 0;
}

export function useLogs() {
  return {
    logs,
    logLevel,
    logBox,
    copied: logCopy.copied,
    refreshLogs,
    copyLogs,
    clearLogs,
  };
}
