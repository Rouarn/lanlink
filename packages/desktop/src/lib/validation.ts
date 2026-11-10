import { RELAY_SCHEMES } from "./labels";

/** 解析服务地址即时校验：空视为沿用内置默认值，不算错误；只接受 http / https */
export function validateResolverUrl(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) return "";
  if (/\s/.test(trimmed)) return "解析服务地址中间不能有空格";
  const match = /^(https?):\/\/([^/?#]+)(\/.*)?$/i.exec(trimmed);
  if (!match) {
    if (/^[a-z][a-z0-9+.-]*:/i.test(trimmed) && !/^https?:/i.test(trimmed)) {
      return "解析服务地址只支持 http:// 或 https://";
    }
    return "解析服务地址要以 http:// 或 https:// 开头";
  }
  const authority = match[2];
  if (!authority) return "解析服务地址缺少主机名";
  const port = authority.includes(":") ? authority.split(":").pop() ?? "" : "";
  if (port && (!/^\d+$/.test(port) || Number(port) < 1 || Number(port) > 65535)) {
    return `解析服务端口「${port}」不合法，应为 1-65535`;
  }
  return "";
}

/** 中继地址列表校验：每行一条，协议前缀、主机名与端口都要合法 */
export function validateRelayList(text: string): string {
  const lines = text
    .split(/[\n,;]/)
    .map((line) => line.trim())
    .filter(Boolean);
  for (const line of lines) {
    if (/\s/.test(line)) return `中继地址「${line}」中间不能有空格`;
    if (!RELAY_SCHEMES.some((scheme) => line.toLowerCase().startsWith(scheme))) {
      return `中继地址「${line}」缺少协议前缀，例如 tcp://你的中继地址:11010`;
    }
    const hostPort = line.split("://")[1]?.split("/")[0] ?? "";
    if (!hostPort) return `中继地址「${line}」缺少主机名`;
    const ipv6 = hostPort.startsWith("[");
    const host = ipv6 ? hostPort.slice(1, hostPort.indexOf("]")) : hostPort.split(":")[0];
    const port = ipv6
      ? hostPort.slice(hostPort.indexOf("]") + 1).replace(/^:/, "")
      : hostPort.includes(":")
        ? hostPort.split(":").pop() ?? ""
        : "";
    if (!host) return `中继地址「${line}」缺少主机名`;
    if (port && (!/^\d+$/.test(port) || Number(port) < 1 || Number(port) > 65535)) {
      return `中继地址「${line}」的端口「${port}」不合法，应为 1-65535`;
    }
  }
  return "";
}
