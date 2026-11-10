import { computed, ref, watch } from "vue";

export type ThemeMode = "system" | "light" | "dark";
export type ThemeName = "light" | "dark";

/** 主题存储键：index.html 的内联脚本用它做首帧兜底，两边必须一致。 */
const STORAGE_KEY = "lanlink.theme";

const query = window.matchMedia("(prefers-color-scheme: dark)");

function readMode(): ThemeMode {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === "light" || saved === "dark") return saved;
  } catch {
    // 读不到就跟随系统
  }
  return "system";
}

const mode = ref<ThemeMode>(readMode());
const systemDark = ref(query.matches);

query.addEventListener("change", (event) => {
  systemDark.value = event.matches;
});

const resolved = computed<ThemeName>(() =>
  mode.value === "system" ? (systemDark.value ? "dark" : "light") : mode.value,
);

watch(
  resolved,
  (theme) => {
    const root = document.documentElement;
    root.dataset.theme = theme;
    root.style.colorScheme = theme;
  },
  { immediate: true },
);

function setMode(next: ThemeMode): void {
  mode.value = next;
  try {
    if (next === "system") localStorage.removeItem(STORAGE_KEY);
    else localStorage.setItem(STORAGE_KEY, next);
  } catch {
    // 存不下就只在本次会话生效
  }
}

export function useTheme() {
  return { mode, resolved, setMode };
}
