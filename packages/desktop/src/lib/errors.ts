import type { UiError } from "../types";

interface Rule {
  test: RegExp;
  title: string;
  detail: string;
}

/**
 * 把后端与网络层的原始错误翻成人话。
 * 原则：标题说「发生了什么」，detail 说「下一步能做什么」，
 * 原始字符串只进 technical，供「技术信息」折叠区查看。
 */
const RULES: Rule[] = [
  {
    // Windows 加载阶段就退出的退出码（0xC0000135 缺 DLL 等）走这条：
    // 重试没有意义，必须重装或补文件，所以放在最前面，别被「找不到 / 解析」之类的规则吃掉。
    test: /运行库|DLL|0xC0000/i,
    title: "网络组件没能启动",
    detail:
      "安装包里的网络组件缺文件或版本不对（常见于安装不完整、被杀毒软件拦截）。重装最新版本通常能修好；还不行就把下面的技术信息发给开发者。",
  },
  {
    test: /404|不存在|找不到|not\s*found|no such room|无效的房间码/i,
    title: "没找到这个房间",
    detail: "房间码可能少复制了字符，或者房间已经关了。让对方重新发一次就好。",
  },
  {
    test: /解析|resolve|dns|network|网络|请求失败|fetch|超时|timeout|timed out|unreachable/i,
    title: "解析服务连不上",
    detail: "先检查这台上网是否正常，再试一次。也可以让对方发「完整房间码」，它不依赖解析服务。",
  },
  {
    test: /端口|port|address already in use|bind|占用/i,
    title: "端口被占用了",
    detail: "可能有上次没退干净的进程占着端口。完全退出 LanLink 再打开，或者重启电脑后再试。",
  },
  {
    test: /授权|拒绝|denied|permission|管理员|elevat|uac|access is denied/i,
    title: "没有拿到管理员权限",
    detail: "创建虚拟网卡需要管理员权限。请在系统弹窗里点「允许」，或在设置里重新检查运行权限后再连接。",
  },
  {
    test: /中继|relay/i,
    title: "连不上中继服务器",
    detail: "检查中继地址是否写对、服务器是否在线。朋友的完整房间码里自带中继，也可以先用它加入。",
  },
  {
    test: /房间码|room code|code/i,
    title: "房间码没法用",
    detail: "确认复制的是完整的一串（短码形如 ROOM-XXXX，完整码以 LL1. 开头），中间不要带空格。",
  },
];

const FALLBACK: Rule = {
  test: /.*/,
  title: "这次没连上",
  detail: "再试一次通常就好了。如果一直这样，打开活动日志看看最后几行，再把技术信息发给房主。",
};

/** 生成面向用户的错误；raw 只进技术信息，不出现在主文案里 */
export function humanizeError(raw: unknown, hint?: string | null): UiError {
  const text = String(raw ?? "").trim();
  const rule = RULES.find((item) => item.test.test(text)) ?? FALLBACK;
  return {
    title: rule.title,
    detail: rule.detail,
    technical: [text, hint?.trim()].filter(Boolean).join("\n"),
  };
}
