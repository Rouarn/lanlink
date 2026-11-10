import { computed, ref } from "vue";
import { ipc } from "../lib/ipc";
import { validateRelayList, validateResolverUrl } from "../lib/validation";
import { info } from "../stores/app";
import type { Settings } from "../types";

const relays = ref<string[]>([]);
const relayText = ref("");
const resolverUrl = ref("");
const settingsError = ref("");
const settingsSaved = ref(false);
/** 《我的世界》局域网发现开关（独立保存，切换即时生效） */
const lanDiscovery = ref(true);
const lanPending = ref(false);
const lanError = ref("");
/** 配置有改动，重连后才生效 */
const relayDirty = ref(false);

let savedTimer: number | undefined;

const relayError = computed(() => validateRelayList(relayText.value));
const resolverError = computed(() => validateResolverUrl(resolverUrl.value));
/** 保存前的整体校验：中继与解析服务地址都要合法 */
const formError = computed(() => relayError.value || resolverError.value);
const configured = computed(() => relays.value.length > 0);
const relayLabel = computed(() => relays.value.join("\n"));
/** 解析服务地址是否仍是内置默认值 */
const resolverIsDefault = computed(() => {
  const fallback = info.value?.default_resolver_base_url ?? "";
  return (
    Boolean(fallback) &&
    resolverUrl.value.trim().replace(/\/+$/, "") === fallback.replace(/\/+$/, "")
  );
});

function applySettings(settings: Settings): void {
  relays.value = settings.relays;
  relayText.value = settings.relays.join("\n");
  resolverUrl.value = settings.resolver_base_url ?? "";
  lanDiscovery.value = settings.lan_discovery;
}

/** 切换局域网发现：立即落盘，连接中会立刻停/起中继。 */
async function setLanDiscovery(enabled: boolean): Promise<void> {
  if (lanPending.value) return;
  lanPending.value = true;
  lanError.value = "";
  try {
    await ipc.setLanDiscovery(enabled);
    lanDiscovery.value = enabled;
  } catch (err) {
    lanError.value = String(err);
  } finally {
    lanPending.value = false;
  }
}

async function loadSettings(): Promise<void> {
  applySettings(await ipc.getSettings());
}

async function saveSettings(): Promise<void> {
  settingsError.value = "";
  settingsSaved.value = false;
  if (formError.value) {
    settingsError.value = formError.value;
    return;
  }
  try {
    applySettings(
      await ipc.saveSettings({
        relays: relayText.value,
        resolverBaseUrl: resolverUrl.value,
      }),
    );
    settingsSaved.value = true;
    relayDirty.value = false;
    if (savedTimer) window.clearTimeout(savedTimer);
    savedTimer = window.setTimeout(() => {
      settingsSaved.value = false;
    }, 1800);
  } catch (err) {
    settingsError.value = String(err);
  }
}

/** 把解析服务地址恢复成内置默认值 */
function restoreDefaultResolver(): void {
  const fallback = info.value?.default_resolver_base_url;
  if (!fallback) return;
  resolverUrl.value = fallback;
  relayDirty.value = true;
}

/** 配置已被保存，清除「需要重连」提示 */
function markClean(): void {
  relayDirty.value = false;
}

export function useSettings() {
  return {
    relays,
    relayText,
    resolverUrl,
    settingsError,
    settingsSaved,
    lanDiscovery,
    lanPending,
    lanError,
    relayDirty,
    relayError,
    resolverError,
    formError,
    configured,
    relayLabel,
    resolverIsDefault,
    loadSettings,
    saveSettings,
    setLanDiscovery,
    restoreDefaultResolver,
    markClean,
  };
}
