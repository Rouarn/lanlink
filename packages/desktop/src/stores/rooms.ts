import { computed, ref } from "vue";
import { ipc } from "../lib/ipc";
import { humanizeError } from "../lib/errors";
import { useApp } from "./app";
import { useSettings } from "./settings";
import { useConnection } from "./connection";
import type { ActiveRoom, Room, UiError } from "../types";

/** 房间与最近房间。只负责「房间是什么」，连接过程交给 connection store。 */
const { toast, go, goRoom } = useApp();
const settings = useSettings();
const connection = useConnection();

const room = ref<Room | null>(null);
const recent = ref<Room[]>([]);
const roomName = ref("");
const joinCode = ref("");
const preview = ref<Room | null>(null);
const previewCode = ref("");
const previewError = ref<UiError | null>(null);
const creating = ref(false);
const longCode = ref("");
const longError = ref("");
const lastAction = ref<"create" | "join" | "reconnect" | null>(null);

const activeRoom = computed<ActiveRoom | null>(() => {
  const current = room.value;
  if (!current) return null;
  return {
    code: current.code,
    name: current.name,
    role: current.role,
    kind: current.kind,
    registration: current.registration,
  };
});

const isHost = computed(() => room.value?.role === "host");

/** 短码没注册上时，必须提示改发完整码 */
const codeWarning = computed(() => {
  const current = room.value;
  if (!current || current.role !== "host") return "";
  if (current.kind === "short" && current.registration === "failed") {
    return "短码没注册上，发给朋友请用完整房间码。";
  }
  return "";
});

async function loadRecent(): Promise<void> {
  recent.value = await ipc.recentRooms();
}

async function create(): Promise<void> {
  if (!settings.configured.value) {
    go("settings");
    toast("先填一条中继服务器，再创建房间");
    return;
  }
  creating.value = true;
  connection.dismissError();
  try {
    const created = await ipc.createRoom(roomName.value.trim() || "我的房间");
    room.value = created;
    lastAction.value = "create";
    await connection.connect(created.code);
  } catch (caught) {
    connection.fail(caught);
  } finally {
    creating.value = false;
  }
}

async function lookup(): Promise<void> {
  const code = joinCode.value.trim();
  previewError.value = null;
  if (!code) {
    previewError.value = {
      title: "还没填房间码",
      detail: "把朋友发来的房间码粘进来，短码和完整码都可以。",
      technical: "",
    };
    return;
  }
  try {
    preview.value = await connection.withResolving(() => ipc.resolveCode(code));
    previewCode.value = code;
  } catch (caught) {
    preview.value = null;
    previewError.value = humanizeError(caught);
  }
}

async function join(): Promise<void> {
  if (!preview.value) return;
  room.value = preview.value;
  lastAction.value = "join";
  await connection.connect(previewCode.value);
}

async function reconnect(target: Room): Promise<void> {
  room.value = target;
  lastAction.value = "reconnect";
  await connection.connect(target.code);
}

async function retry(): Promise<void> {
  if (room.value) {
    await connection.connect(room.value.code);
    return;
  }
  if (lastAction.value === "create") {
    await create();
    return;
  }
  await lookup();
  if (preview.value) await join();
}

async function remove(code: string): Promise<void> {
  await ipc.forgetRoom(code);
  recent.value = recent.value.filter((item) => item.code !== code);
}

async function clearRecent(): Promise<void> {
  await ipc.clearRooms();
  recent.value = [];
}

async function exportLong(): Promise<void> {
  if (!room.value) return;
  longError.value = "";
  try {
    longCode.value = await ipc.exportRoomCode(room.value.code);
  } catch (caught) {
    longError.value = humanizeError(caught).title;
  }
}

function reset(): void {
  room.value = null;
  preview.value = null;
  previewCode.value = "";
  previewError.value = null;
  longCode.value = "";
  longError.value = "";
  lastAction.value = null;
}

function goJoin(): void {
  goRoom("join");
}

function goCreate(): void {
  goRoom("create");
}

export function useRooms() {
  return {
    room,
    recent,
    roomName,
    joinCode,
    preview,
    previewCode,
    previewError,
    creating,
    longCode,
    longError,
    activeRoom,
    isHost,
    codeWarning,
    loadRecent,
    create,
    lookup,
    join,
    reconnect,
    retry,
    remove,
    clearRecent,
    exportLong,
    reset,
    goJoin,
    goCreate,
  };
}
