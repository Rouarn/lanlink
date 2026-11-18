<!-- 本文件为随安装包分发的副本，与仓库根目录 THIRD_PARTY_NOTICES.md 保持一致 -->

# 第三方组件声明 / Third-Party Notices

LanLink 自身代码以 MIT 许可发布（见 [LICENSE](./LICENSE)）。发布包中还会调用下列第三方组件，
它们各自保留原有版权与许可，不受 LanLink 的 MIT 许可影响。

| 组件 | 版本 | 许可证 | 上游 | 是否修改 |
| --- | --- | --- | --- | --- |
| EasyTier（`easytier-core` / `easytier-cli`） | v2.6.4 | LGPL-3.0 | https://github.com/EasyTier/EasyTier | 未修改，原样使用官方 release 二进制 |
| EasyTier C ABI 库（`libeasytier.so`，仅移动端构建脚本使用） | v2.6.4 源码 | LGPL-3.0 | https://github.com/EasyTier/EasyTier | 未修改，由 `scripts/fetch-easytier.sh` 从上游源码编译 |

## 使用方式

LanLink 以**独立进程**调用 `easytier-core`（macOS / Windows），通过 `easytier-cli` 读取 peer 列表；
移动端通过 C ABI 调用 `libeasytier.so`。上游源码与二进制均未被修改。

## LGPL-3.0 义务

分发包含上述组件的安装包时，需要：

1. 保留 EasyTier 的版权声明与本声明；
2. 随包附带 LGPL-3.0 许可证全文（https://www.gnu.org/licenses/lgpl-3.0.txt）；
3. 告知用户可从上游获取对应源码：https://github.com/EasyTier/EasyTier/tree/v2.6.4 ；
4. 如对 EasyTier 做了修改，需一并公开修改部分（本项目未修改）。

> 本文件不是法律意见；涉及商业分发时请自行复核上游许可证要求。

最后更新：2026-11-18（对应 EasyTier v2.6.4）
