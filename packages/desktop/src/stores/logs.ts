import { computed, ref } from "vue";
import { ipc } from "../lib/ipc";
import { copyPlainText } from "../lib/clipboard";
import { useApp } from "./app";
import type { LogFilter, LogLine } from "../types";

/** 活动日志：一次拉全量，前端按级别过滤，计数才不会被过滤器带偏。 */
const { toast } = useApp();

const all = ref<LogLine[]>([]);
const level = ref<LogFilter>("all");
const loading = ref(false);

function normalize(lower: string): Exclude<LogFilter, "all"> {
  if (lower.startsWith("err") || lower === "fatal" || lower === "panic") return "error";
  if (lower.startsWith("warn")) return "warn";
  return "info";
}

const filtered = computed(() => {
  if (level.value === "all") return all.value;
  return all.value.filter((line) => normalize(line.level.toLowerCase()) === level.value);
});

const counts = computed(() => {
  const base = { all: all.value.length, info: 0, warn: 0, error: 0 };
  for (const line of all.value) base[normalize(line.level.toLowerCase())] += 1;
  return base;
});

const plain = computed(() =>
  filtered.value.map((line) => `${line.time}  ${line.level.toUpperCase()}  ${line.message}`).join("\n"),
);

async function refresh(): Promise<void> {
  loading.value = true;
  try {
    all.value = await ipc.readLogs("all", 400);
  } finally {
    loading.value = false;
  }
}

async function copy(): Promise<void> {
  if (!plain.value) {
    toast("现在还没有日志");
    return;
  }
  await copyPlainText(plain.value);
  toast("日志已复制");
}

async function clear(): Promise<void> {
  await ipc.clearLogs();
  all.value = [];
  toast("日志已清空");
}

export function useLogs() {
  return { all, level, loading, filtered, counts, plain, refresh, copy, clear };
}
