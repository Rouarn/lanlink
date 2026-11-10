/** 把后端时间戳（秒或毫秒）转成「刚刚 / 2 小时前 / 昨天 / 3 天前」 */
export function formatRelative(input: number): string {
  if (!input) return "—";
  // 后端多数用秒；超过 1e12 的按毫秒处理
  const ms = input > 1_000_000_000_000 ? input : input * 1000;
  const diff = Date.now() - ms;
  if (diff < 0) return "刚刚";
  const minute = 60_000;
  const hour = 60 * minute;
  const day = 24 * hour;
  if (diff < minute) return "刚刚";
  if (diff < hour) return `${Math.floor(diff / minute)} 分钟前`;
  if (diff < day) return `${Math.floor(diff / hour)} 小时前`;
  if (diff < 2 * day) return "昨天";
  if (diff < 30 * day) return `${Math.floor(diff / day)} 天前`;
  return new Date(ms).toLocaleDateString("zh-CN");
}
