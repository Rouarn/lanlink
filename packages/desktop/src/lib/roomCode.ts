/**
 * 判断一段文本像不像房间码。
 * 短码形如 ROOM-8K3M（本机可解析），完整码以 LL1. 开头且自带配置。
 * 只做轻量识别：宁可漏判，也不要把聊天记录塞进输入框。
 */
export function looksLikeRoomCode(text: string): boolean {
  const value = text.trim();
  if (value.length < 6 || value.length > 4096) return false;
  if (/\s/.test(value)) return false;
  return /^ROOM-[A-Za-z0-9-]+$/.test(value) || /^LL1\./.test(value);
}
