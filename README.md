<div align="center">

# wisp-depmap-agent

**A local-first AI research workbench for DepMap analysis.**

Explore DepMap mutation, expression, and CRISPR gene-dependency data with reproducible scripts, analysis modules, and an AI research agent.

<a href="https://github.com/Yu-Qiao-sjtu/wisp-science-depmap/releases"><img src="https://img.shields.io/github/v/release/Yu-Qiao-sjtu/wisp-science-depmap" alt="Release"></a>
<a href="https://github.com/Yu-Qiao-sjtu/wisp-science-depmap/releases"><img src="https://img.shields.io/github/downloads/Yu-Qiao-sjtu/wisp-science-depmap/total" alt="Downloads"></a>
<a href="https://github.com/Yu-Qiao-sjtu/wisp-science-depmap/blob/main/LICENSE"><img src="https://img.shields.io/github/license/Yu-Qiao-sjtu/wisp-science-depmap" alt="License"></a>
<a href="https://github.com/Yu-Qiao-sjtu/wisp-science-depmap/stargazers"><img src="https://img.shields.io/github/stars/Yu-Qiao-sjtu/wisp-science-depmap?style=social" alt="Stars"></a>
<br>
<a href="https://github.com/Yu-Qiao-sjtu/wisp-science-depmap/releases"><img src="https://img.shields.io/badge/Windows-supported-0078D4" alt="Windows supported"></a>
<a href="https://github.com/Yu-Qiao-sjtu/wisp-science-depmap/releases"><img src="https://img.shields.io/badge/macOS-supported-000000" alt="macOS supported"></a>
<a href="https://github.com/Yu-Qiao-sjtu/wisp-science-depmap/releases"><img src="https://img.shields.io/badge/Linux-supported-FCC624" alt="Linux supported"></a>

[English](README.md) · [简体中文](README_zh.md) · [Releases](https://github.com/Yu-Qiao-sjtu/wisp-science-depmap/releases)

<img src="docs/assets/app-home.png" alt="wisp-depmap-agent desktop app running a bundled RNA-seq analysis demo" width="100%" />

</div>

Search the literature, run Python and R, query ~80 scientific databases, and
keep the trail — figures, runs, decisions, and drafts — in one project. Your
data, conversations, and credentials stay on your machines.

Bring your own model. Keep your science.

## What you can do

**An agent that does the work**

Bring OpenAI-compatible or Anthropic models, or drive Codex / Claude Code over
ACP. The agent reads and writes project files, runs shell, and loads reusable
Skills (`SKILL.md`) without flooding the prompt. Approval gates stay on unless
you opt into Full Permission.

**Compute from laptop to remote servers**

Persistent Python and R kernels keep variables across cells and turns; each
conversation gets its own isolated kernel, so parallel sessions never share
state. Register local, WSL, and SSH hosts once; probe hardware; submit
long **Runs** with live logs. Keys live in the OS keyring, never in SQLite.

**Built for science**

PubMed, GEO, and ~80 other databases through bundled MCP servers. Offline
previews for notebooks, PDFs, Office files, and images. Isolated
[explorations](docs/exploration-branches.md) to try a direction without
touching the mainline. A [Publication Workspace](docs/publication-evidence.md)
that freezes manuscript revisions into verifiable Evidence Capsules.

**A workbench that remembers**

Restart and the full history is back. Undo a turn's file edits. Attach
artifacts, files, and runtimes with `@`; search saved sessions with `#`; apply
a skill with `/`. Encrypted [manual sync](docs/project-sync.md) and
[project transfer](docs/project-transfer.md) — nothing syncs in the background.

## Get started

1. Download from [GitHub Releases](https://github.com/Yu-Qiao-sjtu/wisp-science-depmap/releases).
2. Open a bundled demo — no API key needed — to see a full RNA-seq trajectory.
3. Add a model in **Settings → Models** and start a project.

| Platform | Package |
|----------|---------|
| Windows  | Signed MSI / NSIS |
| macOS    | Signed, notarized `.dmg` (Apple Silicon + Intel) |
| Linux    | `.deb` / AppImage (x86_64 + aarch64) |

Setup walkthrough: [basic configuration](docs/basic-configuration.md) ·
[model profiles](docs/model-configuration.md) ·
[ACP agents](docs/acp-agents.md)

Build from source, CLI, and architecture: [development](docs/development.md).

## Documentation

| | |
|---|---|
| **Start** | [Basic setup](docs/basic-configuration.md) · [Models](docs/model-configuration.md) · [ACP agents](docs/acp-agents.md) |
| **Research** | [Explorations](docs/exploration-branches.md) · [Evidence capsules](docs/publication-evidence.md) · [Case studies](docs/case-studies.zh-CN.md) |
| **Projects** | [Transfer](docs/project-transfer.md) · [Sync](docs/project-sync.md) · [Global library](docs/global-library.md) |
| **Compute** | [Terminals](docs/terminal-sessions.md) · [Remote files](docs/remote-file-browser.md) · [Transfers](docs/server-transfers.md) |
| **Extend** | [Skills](docs/skills.md) · [Plugins](docs/feature-plugins.md) · [Delegation](docs/agent-delegation.md) · [Channels](docs/channels.md) · [Browser](docs/real-browser-automation.md) |
| **Develop** | [Development](docs/development.md) · [Headless eval](docs/headless-agent-testing.md) |

Windows code signing by [SignPath.io](https://signpath.io), certificate by the
[SignPath Foundation](https://signpath.org). Third-party notices live in
[development](docs/development.md).

## License

[AGPL-3.0-only](LICENSE), except where a directory notes otherwise. Earlier
releases keep the license published with them.
