import { computed, ref } from "vue";
import { ipc } from "../lib/ipc";
import { activeTab, busy, message } from "../stores/app";
import type { ActiveRoom, Room, RoomRegistration } from "../types";
import { useCopyFeedback } from "./useClipboard";
import { useSettings } from "./useSettings";
import { useTunnel } from "./useTunnel";

const room = ref<Room | null>(null);
const recent = ref<Room[]>([]);
const roomName = ref("");
const joinCode = ref("");
/** 加入前预览：房间码解析出的房间与中继 */
const preview = ref<Room | null>(null);
const previewCode = ref("");
const longCode = ref("");
const longError = ref("");

const roomCodeCopy = useCopyFeedback();
const longCodeCopy = useCopyFeedback();

/** 没有当前房间时一律视为「未尝试注册」，界面据此不显示任何注册提示，
 *  不能把「没有数据」当成「注册失败」。 */
const NO_REGISTRATION: RoomRegistration = "skipped";

const activeRoom = computed<ActiveRoom | null>(() => {
  const { status } = useTunnel();
  if (status.value?.room) {
    return {
      code: status.value.room,
      name: status.value.room_name ?? room.value?.name ?? status.value.room,
      role: status.value.role ?? room.value?.role ?? "member",
      kind: room.value?.kind ?? "short",
      registration: room.value?.registration ?? NO_REGISTRATION,
    };
  }
  return room.value
    ? {
        code: room.value.code,
        name: room.value.name,
        role: room.value.role,
        kind: room.value.kind,
        registration: room.value.registration ?? NO_REGISTRATION,
      }
    : null;
});

async function loadRecent(): Promise<void> {
  recent.value = await ipc.recentRooms();
}

function requireRelay(): boolean {
  if (useSettings().configured.value) return true;
  message.value = "请先在中继设置里填写服务器地址。";
  activeTab.value = "settings";
  return false;
}

/** 清掉跟「本次房间」绑定的临时状态：解析预览、导出的完整码、导出错误。
 *  断开或切到下一个房间时都要清，别让上一轮的结果留在下一屏。 */
function clearRoomExtras(): void {
  preview.value = null;
  previewCode.value = "";
  longCode.value = "";
  longError.value = "";
}

async function createRoom(): Promise<void> {
  if (!requireRelay()) return;
  if (!roomName.value.trim()) {
    message.value = "请先填写房间名";
    return;
  }
  busy.value = true;
  message.value = "";
  clearRoomExtras();
  try {
    const created = await ipc.createRoom(roomName.value.trim());
    room.value = created;
    await useTunnel().connect(created.code);
    await loadRecent();
  } catch (err) {
    message.value = String(err);
  } finally {
    busy.value = false;
  }
}

/** 只解析、不连接：先把房间名与中继显示出来，确认后再连。
 *  加入方不需要本地已配中继——中继来自房间记录本身。 */
async function lookupCode(): Promise<void> {
  const code = joinCode.value.trim();
  if (!code) {
    message.value = "请填写房间码";
    return;
  }
  busy.value = true;
  message.value = "";
  clearRoomExtras();
  try {
    preview.value = await ipc.resolveCode(code);
    previewCode.value = code;
  } catch (err) {
    preview.value = null;
    message.value = String(err);
  } finally {
    busy.value = false;
  }
}

async function joinResolved(): Promise<void> {
  if (!preview.value) return;
  busy.value = true;
  message.value = "";
  try {
    room.value = preview.value;
    await useTunnel().connect(previewCode.value);
    await loadRecent();
    clearRoomExtras();
  } catch (err) {
    message.value = String(err);
  } finally {
    busy.value = false;
  }
}

async function exportLong(): Promise<void> {
  if (!room.value) return;
  longError.value = "";
  try {
    longCode.value = await ipc.exportRoomCode(room.value.code);
  } catch (err) {
    longError.value = String(err);
  }
}

async function copyRoomCode(code: string): Promise<void> {
  await roomCodeCopy.copy(code);
}

async function copyLongCode(): Promise<void> {
  await longCodeCopy.copy(longCode.value);
}

async function reconnect(target: Room): Promise<void> {
  busy.value = true;
  message.value = "";
  clearRoomExtras();
  try {
    room.value = target;
    await useTunnel().connect(target.code);
    await loadRecent();
  } catch (err) {
    message.value = String(err);
  } finally {
    busy.value = false;
  }
}

/** 删除一条历史房间，后端会立刻落盘，重启后不再出现。 */
async function removeRoom(code: string): Promise<void> {
  try {
    await ipc.forgetRoom(code);
    await loadRecent();
  } catch (err) {
    message.value = String(err);
  }
}

/** 清空全部历史房间。 */
async function clearRooms(): Promise<void> {
  try {
    await ipc.clearRooms();
    await loadRecent();
  } catch (err) {
    message.value = String(err);
  }
}

/** 断开后清掉当前房间与它的全部临时状态，界面回到未连接；
 *  最近房间列表不受影响。 */
function resetActiveRoom(): void {
  room.value = null;
  clearRoomExtras();
}

export function useRooms() {
  return {
    room,
    recent,
    roomName,
    joinCode,
    preview,
    previewCode,
    longCode,
    longError,
    activeRoom,
    copied: roomCodeCopy.copied,
    longCopied: longCodeCopy.copied,
    loadRecent,
    createRoom,
    lookupCode,
    joinResolved,
    exportLong,
    copyRoomCode,
    copyLongCode,
    reconnect,
    removeRoom,
    clearRooms,
    resetActiveRoom,
  };
}
