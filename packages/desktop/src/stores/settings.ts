import { computed, ref } from "vue";
import { ipc } from "../lib/ipc";
import { useApp } from "./app";
import type { Settings, TunnelStatus } from "../types";

/** 与 Rust 侧 broadcast::default_ports 保持一致：一键开启通用转发时用它 */
export const DEFAULT_BROADCAST_PORTS = [
  2300, 2301, 6112, 6113, 6114, 6073, 7777, 7778, 27015, 27016, 27017, 27960, 47624,
];

const { info, toast } = useApp();

const relayText = ref("");
const relays = ref<string[]>([]);
const resolverUrl = ref("");
const defaultResolver = ref("");
const lanDiscovery = ref(true);
const broadcastPorts = ref<number[]>([]);
const dirty = ref(false);
const loaded = ref(false);
const saving = ref(false);
const saveError = ref("");
const lanBusy = ref(false);
const lanError = ref("");

/** 至少有一条中继才算「配置好了」 */
const configured = computed(() => relays.value.length > 0);

/** 实际生效的解析服务：留空表示用内置地址 */
const resolverEffective = computed(() => resolverUrl.value.trim() || defaultResolver.value);

const resolverIsDefault = computed(
  () => !resolverUrl.value.trim() || resolverUrl.value.trim() === defaultResolver.value,
);

/** 任一自动发现方式打开就算开着，用于顶部的提示 */
const lanOn = computed(() => lanDiscovery.value || broadcastPorts.value.length > 0);

function apply(next: Settings): void {
  relays.value = [...next.relays];
  relayText.value = next.relays.join("\n");
  resolverUrl.value = next.resolver_base_url ?? "";
  lanDiscovery.value = next.lan_discovery;
  broadcastPorts.value = [...next.broadcast_ports];
  defaultResolver.value = info.value?.default_resolver_base_url ?? "";
  dirty.value = false;
}

async function load(): Promise<void> {
  apply(await ipc.getSettings());
  defaultResolver.value = info.value?.default_resolver_base_url ?? "";
  loaded.value = true;
}

function markDirty(): void {
  dirty.value = true;
  saveError.value = "";
}

async function save(): Promise<boolean> {
  saving.value = true;
  saveError.value = "";
  try {
    apply(await ipc.saveSettings({ relays: relayText.value, resolverBaseUrl: resolverUrl.value.trim() }));
    toast(relays.value.length ? "中继已保存" : "已清空中继配置");
    return true;
  } catch (error) {
    saveError.value = String(error);
    return false;
  } finally {
    saving.value = false;
  }
}

/** 首屏只需要一条地址：写入输入框并立即保存 */
async function saveRelay(address: string): Promise<boolean> {
  relayText.value = address.trim();
  return save();
}

async function setLanDiscovery(enabled: boolean): Promise<TunnelStatus | null> {
  lanBusy.value = true;
  lanError.value = "";
  try {
    const status = await ipc.setLanDiscovery(enabled);
    lanDiscovery.value = enabled;
    return status;
  } catch (error) {
    lanError.value = String(error);
    return null;
  } finally {
    lanBusy.value = false;
  }
}

async function setBroadcastPorts(ports: number[]): Promise<TunnelStatus | null> {
  lanBusy.value = true;
  lanError.value = "";
  try {
    const status = await ipc.setBroadcastPorts(ports);
    broadcastPorts.value = [...ports];
    return status;
  } catch (error) {
    lanError.value = String(error);
    return null;
  } finally {
    lanBusy.value = false;
  }
}

/** 返回错误文案，空字符串表示端口可用 */
function validatePort(input: string): string {
  const port = Number(input);
  if (!Number.isInteger(port) || port < 1 || port > 65535) return "端口要是 1 到 65535 之间的整数";
  if (port === 4445 || port === 4448) return "这个端口 LanLink 自己在用，换一个";
  if (broadcastPorts.value.includes(port)) return "这个端口已经在表里了";
  return "";
}

export function useSettings() {
  return {
    relayText,
    relays,
    resolverUrl,
    defaultResolver,
    resolverEffective,
    resolverIsDefault,
    lanDiscovery,
    broadcastPorts,
    dirty,
    loaded,
    saving,
    saveError,
    lanBusy,
    lanError,
    configured,
    lanOn,
    load,
    save,
    saveRelay,
    markDirty,
    setLanDiscovery,
    setBroadcastPorts,
    validatePort,
  };
}
