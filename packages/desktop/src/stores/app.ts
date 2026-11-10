import { computed, ref } from "vue";
import { ipc } from "../lib/ipc";
import type { AppInfo, Platform, RoomScreen, ViewKey } from "../types";

/** 全局界面状态：导航、平台、应用信息、轻提示。连接状态不在这里。 */
const info = ref<AppInfo | null>(null);
/** 安装包版本号（来自 tauri.conf.json），非 Tauri 环境为空串 */
const version = ref("");
const view = ref<ViewKey>("room");
const roomScreen = ref<RoomScreen>("create");
const drawerOpen = ref(false);
const toastText = ref("");
let toastTimer: number | undefined;

export const platform = computed<Platform>(() =>
  info.value?.platform === "windows" ? "win" : "mac",
);

/** 只在有 AppInfo 之后调用一次：设置 data-platform 供样式隔离 */
async function loadInfo(): Promise<void> {
  const [nextInfo, nextVersion] = await Promise.all([ipc.appInfo(), ipc.appVersion()]);
  info.value = nextInfo;
  version.value = nextVersion;
  document.documentElement.dataset.platform = platform.value;
}

export function useApp() {
  function toast(text: string): void {
    toastText.value = text;
    window.clearTimeout(toastTimer);
    toastTimer = window.setTimeout(() => {
      toastText.value = "";
    }, 2200);
  }

  function go(next: ViewKey): void {
    view.value = next;
  }

  function goRoom(screen?: RoomScreen): void {
    if (screen) roomScreen.value = screen;
    view.value = "room";
  }

  function openDrawer(): void {
    drawerOpen.value = true;
  }

  function closeDrawer(): void {
    drawerOpen.value = false;
  }

  return {
    info,
    version,
    view,
    roomScreen,
    drawerOpen,
    toastText,
    platform,
    loadInfo,
    toast,
    go,
    goRoom,
    openDrawer,
    closeDrawer,
  };
}
