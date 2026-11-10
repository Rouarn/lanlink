export type TabKey = "connect" | "peers" | "logs" | "settings";

export interface TabItem {
  key: TabKey;
  label: string;
  /** 16x16 线性图标的内联 SVG 内容 */
  icon: string;
}

export const tabs: TabItem[] = [
  {
    key: "connect",
    label: "连接",
    icon: '<path d="M6.6 9.4 9.4 6.6"/><path d="M7.3 4.6 8.5 3.4a2.6 2.6 0 0 1 3.7 3.7l-1.2 1.2"/><path d="M8.7 11.4 7.5 12.6a2.6 2.6 0 0 1-3.7-3.7l1.2-1.2"/>',
  },
  {
    key: "peers",
    label: "对端",
    icon: '<circle cx="5.6" cy="5.4" r="2.1"/><path d="M1.8 12.8c0-1.9 1.7-3.2 3.8-3.2s3.8 1.3 3.8 3.2"/><circle cx="11.6" cy="4.6" r="1.6"/><path d="M10.4 9.7c.4-.2.9-.3 1.4-.3 1.7 0 3 1.1 3 2.7"/>',
  },
  {
    key: "logs",
    label: "日志",
    icon: '<rect x="2.6" y="2.4" width="10.8" height="11.2" rx="2.2"/><path d="M5.2 5.9h5.6M5.2 8.2h5.6M5.2 10.5h3.2"/>',
  },
  {
    key: "settings",
    label: "设置",
    icon: '<path d="M2.4 5.4h6.1M12.1 5.4h1.5M2.4 10.6h1.5M7.5 10.6h6.1"/><circle cx="10.3" cy="5.4" r="1.7"/><circle cx="5.7" cy="10.6" r="1.7"/>',
  },
];
