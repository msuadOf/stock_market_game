# 📈 Stock Market Game

> 一个浏览器优先的股票模拟经营游戏。从开盘钟声到收盘线，模拟市场、构建策略、管理风险。
>
> **A browser-first stock market simulation game.** Trade, strategize, and manage risk in a simulated market.

[![Status: Pre-alpha](https://img.shields.io/badge/status-pre--alpha-orange)]()
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Test-Driven](https://img.shields.io/badge/dev-TDD-success)](docs/testing.md)

---

## 🎯 项目愿景

一款**可玩、可学、可扩展**的股票市场模拟游戏：

- 🌐 **Web 优先** — 打开浏览器即玩，零安装
- 🔌 **后端可选** — 单机也能完整游玩；接上后端即可联机 / 持久化 / 远程部署
- 🖥️ **桌面版** — 通过 Tauri 打包为原生应用

### 路线图（Roadmap）

| 阶段 | 目标 | 后端 | 状态 |
|------|------|------|------|
| **Stage 1** | 纯前端单机可玩（本地存档） | ❌ 不需要 | 🔨 规划中 |
| **Stage 2** | 前后端分离，后端可选（联机 / 远程部署） | ✅ 可选 | 📋 计划 |
| **Stage 3** | Tauri 桌面应用 | ✅ 可选 | 📋 计划 |

> 详细路线见 [`docs/roadmap.md`](docs/roadmap.md)（待补）。

---

## 🚧 当前状态：**框架搭建阶段**

> 本仓库**尚未编写业务代码**。当前阶段专注于建立 AI 协作框架、工程规范与技术路线，
> 确保后续开发（无论人类还是 AI）都能在一致的约束下推进。

**已就位：**
- ✅ AI 协作规范（`CLAUDE.md`、`AGENTS.md`）
- ✅ 工程原则（TDD、防御式编程、错误处理哲学）
- ✅ 架构与技术路线文档
- ✅ ADR（架构决策记录）机制 + 开放问题清单
- ✅ GitHub 协作模板（Issue / PR 模板）

**下一步：** 敲定 [`docs/decisions/`](docs/decisions/) 中的开放技术决策后，启动 Stage 1。

---

## 🤝 参与贡献

本项目由 AI（Claude）与人类协作开发。无论你是人类还是 AI agent，请先阅读：

1. **[`CLAUDE.md`](CLAUDE.md)** / **[`AGENTS.md`](AGENTS.md)** — 协作守则（**必读**）
2. **[`docs/principles.md`](docs/principles.md)** — 工程原则
3. **[`docs/testing.md`](docs/testing.md)** — TDD 工作流
4. **[`docs/decisions/`](docs/decisions/)** — 架构决策记录（决策前的上下文都在这）

**核心铁律：**
- 🧪 **测试先行** — 先写失败测试，再写实现（TDD）
- 🛡️ **防御式编程** — 错误必须显式暴露给用户，绝不静默吞掉
- 📐 **不静默 fallback** — 宁可让程序崩溃并展示细节，也不要悄悄用默认值掩盖问题

---

## 📜 许可证

[MIT](LICENSE) © 2026 msuad

> ⚠️ 许可证类型为初步选择，最终以 [`docs/decisions/`](docs/decisions/) 中的 ADR 为准。
