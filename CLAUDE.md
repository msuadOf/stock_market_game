# CLAUDE.md

> 本文件是给 **Claude（以及所有 AI 协作者）** 的项目导航与协作守则。
> 人类贡献者请读其姊妹文件 [`AGENTS.md`](AGENTS.md)（内容一致，措辞面向人类）。
>
> **无论你是谁，开工前必读本文件 + [`docs/principles.md`](docs/principles.md)。**

---

## 0. 一句话项目概述

**股票模拟游戏，Web 优先**：纯前端单机可玩 → 前后端分离（后端可选）→ Tauri 桌面版。
游戏核心逻辑必须可独立测试、与渲染层解耦。

## 1. 三条不可违反的铁律（The Three Iron Rules）

> 违反任何一条视为工作未完成。AI 在每次提交前应自检。

### 🧪 铁律一：测试驱动（TDD）—— 先红后绿

- **永远先写测试，再写实现。** 没有"先写完代码再补测试"。
- 工作循环：**红**（写一个失败的测试）→ **绿**（写最少代码让它过）→ **重构**。
- 如果一段代码"难以测试"，说明它的设计有问题——先重构设计，而不是绕过测试。
- 详见 [`docs/testing.md`](docs/testing.md)。

### 🛡️ 铁律二：防御式编程 —— 错误必须显式可见

- **绝不静默吞掉错误。** 空 `catch`、`catch { return null }`、`?? 默认值` 掩盖异常——禁止。
- 任何错误都必须**显式展示给用户**：清晰的错误信息 + 可复现的细节（发生了什么 / 在哪 / 为什么 / 如何反馈）。
- 宁可让程序**带着详情崩溃/报错**，也不要**悄悄用默认值继续运行**而产生不可解释的行为。
- 详见 [`docs/error-handling.md`](docs/error-handling.md)。

### 📐 铁律三：最小惊讶 + 诚实反馈

- 函数/组件只做名字承诺的事。副作用要显式。
- 报告工作结果要**诚实**：测试失败就如实说失败；跳过了步骤就明说跳过；没验证就别说"已完成"。
- 不要为了"显得完成了"而编造 API、文件路径或测试结果。

## 2. 仓库地图（你在哪 / 该去哪）

```
stock_market_game/
├── CLAUDE.md              ← 你在这里：AI 协作守则
├── AGENTS.md              ← 人类协作守则
├── CONTRIBUTING.md        ← 贡献流程（分支、提交、PR）
├── README.md
├── docs/
│   ├── principles.md      ← 工程原则（必读）
│   ├── testing.md         ← TDD 工作流
│   ├── error-handling.md  ← 错误处理哲学
│   ├── architecture.md    ← 分层架构与依赖方向
│   ├── tech-stack.md      ← 技术栈选型（含未决项）
│   ├── decisions/         ← ADR：架构决策记录（决策看这里）
│   │   ├── 0000-template.md
│   │   └── 0001-record-architecture-decisions.md
│   └── open-questions.md  ← 待敲定的开放问题
├── .github/               ← Issue / PR 模板
└── （源码目录将在 Stage 1 启动时创建：apps/web, packages/engine 等）
```

> ⚠️ 当前处于**框架搭建阶段**，尚无 `apps/`、`packages/` 源码目录。架构布局见
> [`docs/architecture.md`](docs/architecture.md) 的"目标结构"章节。

## 3. AI 工作协议（How to work）

### 3.1 动手前

1. **定位**：先读 [`docs/decisions/`](docs/decisions/) 和 [`docs/open-questions.md`](docs/open-questions.md)，
   确认相关决策已敲定。若未敲定 → **不要擅自定技术路线**，标记为开放问题并上报。
2. **理解边界**：确认你要改的层在哪（engine / web / server / desktop），遵守依赖方向（见 architecture.md）。
3. **小步快走**：一次只解决一个小问题，便于回滚和评审。

### 3.2 写代码时

1. **先写测试**（红），再写实现（绿），再重构。提交粒度 = 一个红绿循环。
2. **防御式**：对外部输入（用户输入、网络、存档文件、LocalStorage）一律校验 + 显式错误。
3. **匹配上下文**：新代码的命名、注释密度、风格要与周围代码一致。
4. **不引入未批准的依赖**：新增依赖需在 PR 中说明理由，重大依赖需先有 ADR。

### 3.3 动手后

1. **跑测试**：确保全绿。红了就修，修不好就如实报告，不要"调整测试让它通过"。
2. **诚实汇报**：哪些完成了、哪些跳过了、哪些失败了——逐项说明，附上命令输出。
3. **更新文档**：若你的改动影响架构/约定，更新对应文档；重大决策补一条 ADR。

### 3.4 提交信息规范

使用 [Conventional Commits](https://www.conventionalcommits.org/)：

```
<type>(<scope>): <subject>

<body 可选，说明 why>

<footer 可选，如 BREAKING CHANGE、关联 issue>
```

- `type`：`feat | fix | docs | test | refactor | chore | perf | build | ci`
- `scope`：`engine | web | server | desktop | docs | test`
- 示例：`test(engine): 为价格波动模型增加边界用例`、`feat(web): 接入股票列表视图`

### 3.5 GitHub 操作规范

**本项目所有 GitHub 操作一律使用 `gh` CLI**（本机已装 2.95）。不要用网页、不要用 git remote 手动拼接、不要用其他封装工具。

| 操作 | 命令 |
|------|------|
| 建远程仓库并推送 | `gh repo create <name> --public/--private --source=. --remote=origin --push` |
| 看仓库/PR/Issue | `gh repo view` / `gh pr view` / `gh issue view` |
| 开 / 评审 / 合并 PR | `gh pr create` / `gh pr review` / `gh pr merge` |
| 开 / 关 Issue | `gh issue create` / `gh issue close` |
| Actions 状态 | `gh run list` / `gh run view` |
| 发布 Release | `gh release create` |

约定：
- 推送、建公开仓库、发版属于**对外不可逆**操作 → 执行前须人类确认（见 §4），除非已获明确授权。
- 远程名固定 `origin`，默认分支 `main`。
- 涉及机密（token、密钥）一律经 `gh secret set`，**绝不**写入代码或提交。

## 4. 何时必须停下来问人类

> AI 不应假装拥有它没有的权限或信息。遇到以下情况，**停下来问**，而不是猜：

- 需要敲定一项**开放的技术决策**（见 `docs/open-questions.md`）。
- 需要引入**新的核心依赖**或改变技术栈。
- 需要执行**不可逆 / 对外**的操作（推送到远程、建 GitHub 仓库、发版、删数据）。
- 测试持续失败且原因不明（不要为了让它过而篡改测试）。
- 需求模糊到有多种合理实现，且取舍会显著影响后续。

## 5. 绝对不要做（Never list）

- ❌ 不要为了让测试通过而修改/删除/弱化测试的断言。
- ❌ 不要静默 `catch` 错误或用 `try/catch` + 默认值掩盖异常。
- ❌ 不要在未读 `docs/decisions/` 的情况下擅自定技术路线。
- ❌ 不要在同一个提交里混入多个无关改动。
- ❌ 不要编造不存在的 API、文件、函数名或测试结果。
- ❌ 不要未经确认就 push 到远程、创建公开仓库或发布版本。
- ❌ 不要用非 `gh` 的方式做 GitHub 操作（网页 / 手动 git remote / 其他封装工具）。

---

## 附：相关文档导航

| 主题 | 文档 |
|------|------|
| 工程原则总纲 | [`docs/principles.md`](docs/principles.md) |
| TDD 怎么做 | [`docs/testing.md`](docs/testing.md) |
| 错误怎么处理 | [`docs/error-handling.md`](docs/error-handling.md) |
| 架构分层 | [`docs/architecture.md`](docs/architecture.md) |
| 技术栈 | [`docs/tech-stack.md`](docs/tech-stack.md) |
| 历史决策 | [`docs/decisions/`](docs/decisions/) |
| 待定问题 | [`docs/open-questions.md`](docs/open-questions.md) |
