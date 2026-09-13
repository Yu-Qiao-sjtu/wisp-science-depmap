<div align="center">

# wisp-depmap-agent

**面向 DepMap 分析的本地优先 AI 科研工作台。**

通过可复现脚本、分析模块和 AI 科研智能体，探索 DepMap 的突变、表达量与 CRISPR 基因依赖数据。

<a href="https://github.com/Yu-Qiao-sjtu/wisp-science-depmap/releases"><img src="https://img.shields.io/github/v/release/Yu-Qiao-sjtu/wisp-science-depmap" alt="Release"></a>
<a href="https://github.com/Yu-Qiao-sjtu/wisp-science-depmap/releases"><img src="https://img.shields.io/github/downloads/Yu-Qiao-sjtu/wisp-science-depmap/total" alt="下载量"></a>
<a href="https://github.com/Yu-Qiao-sjtu/wisp-science-depmap/blob/main/LICENSE"><img src="https://img.shields.io/github/license/Yu-Qiao-sjtu/wisp-science-depmap" alt="许可证"></a>
<a href="https://github.com/Yu-Qiao-sjtu/wisp-science-depmap/stargazers"><img src="https://img.shields.io/github/stars/Yu-Qiao-sjtu/wisp-science-depmap?style=social" alt="Stars"></a>
<br>
<a href="https://github.com/Yu-Qiao-sjtu/wisp-science-depmap/releases"><img src="https://img.shields.io/badge/Windows-supported-0078D4" alt="支持 Windows"></a>
<a href="https://github.com/Yu-Qiao-sjtu/wisp-science-depmap/releases"><img src="https://img.shields.io/badge/macOS-supported-000000" alt="支持 macOS"></a>
<a href="https://github.com/Yu-Qiao-sjtu/wisp-science-depmap/releases"><img src="https://img.shields.io/badge/Linux-supported-FCC624" alt="支持 Linux"></a>

[English](README.md) · [简体中文](README_zh.md) · [Releases](https://github.com/Yu-Qiao-sjtu/wisp-science-depmap/releases)

<img src="docs/assets/app-home.png" alt="wisp-depmap-agent 桌面应用正在运行内置的 RNA-seq 分析演示" width="100%" />

</div>

检索文献、运行 Python 与 R、查询约 80 个科学数据库，把图、Run、判断和稿件
留在同一个项目里。数据、会话和凭据都在你自己的机器上。

模型你自己选。数据留在本地。

## 你可以用它做什么

**能真正干活的 Agent**

接入 OpenAI 兼容或 Anthropic 模型，也可以通过 ACP 驱动 Codex / Claude Code。
Agent 读写项目文件、执行 shell，并按需加载 Skills（`SKILL.md`），不会把目录
塞进提示词。默认走审批门控，需要时再开 Full Permission。

**从笔记本到远程服务器**

持久化 Python / R 内核，变量在同一会话内跨 cell 与轮次保留；每个会话拥有独立
内核，并行会话互不干扰。本地、WSL、SSH 主机注册
一次即可探测硬件、提交带实时日志的长 **Run**。密钥只进系统密钥环，不进 SQLite。

**为科研而生**

通过内置 MCP 访问 PubMed、GEO 等约 80 个数据库。离线预览 notebook、PDF、Office
和图片。[探索分支](docs/exploration-branches.zh-CN.md)让你试一条方向而不改主线。
[出版工作区](docs/publication-evidence.md)把稿件修订冻成可验证的证据胶囊。

**会记忆的工作台**

重启后完整历史还在。一键撤销某一轮的文件改动。`@` 附加产物与运行时，`#` 检索
已保存会话，`/` 套用 skill。[加密手动同步](docs/project-sync.zh-CN.md)与
[项目迁移](docs/project-transfer.md)——绝不在后台偷跑。

## 开始使用

1. 从 [GitHub Releases](https://github.com/Yu-Qiao-sjtu/wisp-science-depmap/releases) 下载。
2. 打开内置演示（无需 API Key），看完整的 RNA-seq 轨迹。
3. 在 **设置 → 模型** 中添加模型，然后开一个项目。

| 平台 | 安装包 |
|------|--------|
| Windows | 已签名 MSI / NSIS |
| macOS | 已签名并公证的 `.dmg`（Apple Silicon + Intel） |
| Linux | `.deb` / AppImage（x86_64 + aarch64） |

上手教程：[快速开始](docs/wechat/wisp-science-quick-start.md) ·
[基础配置](docs/basic-configuration.md) ·
[模型配置](docs/model-configuration.md) ·
[ACP Agents](docs/acp-agents.md)

源码构建、CLI 与架构见[开发指南](docs/development.md)。

## 文档

| | |
|---|---|
| **上手** | [基础配置](docs/basic-configuration.md) · [模型](docs/model-configuration.md) · [ACP](docs/acp-agents.md) |
| **科研** | [探索分支](docs/exploration-branches.zh-CN.md) · [证据胶囊](docs/publication-evidence.md) · [选题库](docs/case-studies.zh-CN.md) |
| **项目** | [迁移](docs/project-transfer.md) · [同步](docs/project-sync.zh-CN.md) · [全局库](docs/global-library.md) |
| **算力** | [终端](docs/terminal-sessions.md) · [远程文件](docs/remote-file-browser.md) · [传输](docs/server-transfers.md) |
| **扩展** | [Skills](docs/skills.md) · [插件](docs/feature-plugins.md) · [委派](docs/agent-delegation.md) · [IM](docs/channels.md) · [浏览器](docs/real-browser-automation.md) |
| **开发** | [开发指南](docs/development.md) · [无头评测](docs/headless-agent-testing.md) |

Windows 代码签名由 [SignPath.io](https://signpath.io) 提供，证书由
[SignPath Foundation](https://signpath.org) 签发。第三方声明见
[开发指南](docs/development.md)。

## 许可证

除另有说明外，采用 [AGPL-3.0-only](LICENSE)。更早发布的版本继续适用其发布时
附带的许可证。
