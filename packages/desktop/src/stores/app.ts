import { computed, ref } from "vue";
import { tabs, type TabKey } from "../constants/navigation";
import { ipc } from "../lib/ipc";
import type { AppInfo } from "../types";

/**
 * 全局界面状态：模块级单例，所有组件共享同一份引用。
 * 只放跨页面共享的最小状态，业务数据由各自的组合式函数维护。
 */
export const activeTab = ref<TabKey>("connect");
export const message = ref("");
export const busy = ref(false);
export const info = ref<AppInfo | null>(null);

export const pageTitle = computed(
  () => tabs.find((tab) => tab.key === activeTab.value)?.label ?? "",
);

export async function loadAppInfo(): Promise<void> {
  info.value = await ipc.appInfo();
}
