# 贡献指南 (Contributing)

> 欢迎贡献！本项目由 AI 与人类协作开发。无论你是谁，下面的流程对所有人都适用。
>
> **开工前必读：** [`AGENTS.md`](AGENTS.md)（或 [`CLAUDE.md`](CLAUDE.md)）+ [`docs/principles.md`](docs/principles.md)。

---

## 0. 三条铁律（先看这个）

1. 🧪 **TDD** — 先写测试，再写实现（[`docs/testing.md`](docs/testing.md)）。
2. 🛡️ **防御式编程** — 错误必须显式暴露，绝不静默吞掉（[`docs/error-handling.md`](docs/error-handling.md)）。
3. 📐 **诚实 + 最小惊讶** — 汇报如实，代码只做承诺的事。

违反任一条的 PR 不会被合并。

## 1. 环境准备

| 工具 | 用途 | 版本要求 |
|------|------|----------|
| Node.js | 前端 | ≥ 20（推荐 LTS） |
| Rust | 核心引擎 / 后端 / Tauri | 稳定版 |
| npm | 包管理 | 随 Node |

> 📌 Go 目前**未**纳入工具链（详见 [`docs/tech-stack.md`](docs/tech-stack.md)）。
> 包管理器：仓库默认使用 **npm**（本机已具备）；是否迁移 pnpm 待定（见开放问题）。

## 2. 分支策略

- `main` — 始终可运行、测试全绿的受保护分支。
- 功能分支命名：`feat/<scope>-<short-desc>`（如 `feat/engine-price-tick`）。
- 修复分支：`fix/<scope>-<short-desc>`。
- 文档：`docs/<short-desc>`。

> ⚠️ 不直接向 `main` 提交。所有改动走 Pull Request。

## 3. 提交信息（Conventional Commits）

```
<type>(<scope>): <subject>

<body 说明 why，不是 what>

<footer：BREAKING CHANGE / 关联 issue>
```

- **type**：`feat | fix | docs | test | refactor | chore | perf | build | ci`
- **scope**：`engine | web | server | desktop | docs | test`
- subject 用祈使句、现在时、首字母小写、不加句号。

**好的例子：**
- `test(engine): 为价格波动模型增加边界用例`
- `feat(web): 接入股票列表视图`
- `refactor(engine): 抽离撮合引擎为独立模块`

**坏的例子：**
- `update code`（无 type、无 scope、无信息量）
- `Fixed some bugs`（过去时、模糊）

## 4. Pull Request 流程

1. **开 Issue 先**（除微小改动）：描述问题 / 需求，确认没人已在做。
2. 从最新的 `main` 切出分支。
3. 按 TDD 循环开发（红 → 绿 → 重构）。
4. 本地确保：测试全绿、lint 通过、build 成功。
5. 开 PR，套用 [`PULL_REQUEST_TEMPLATE`](.github/pull_request_template.md)：
   - 关联 Issue
   - 测试清单（说明你为它写了哪些测试）
   - 是否触及 [`docs/open-questions.md`](docs/open-questions.md) 中的开放问题
   - 是否引入新依赖（需说明理由）
6. 至少一次评审通过 + CI 全绿后合并。

### PR 自检清单

- [ ] 有对应的测试，且测试先于实现编写
- [ ] 所有错误路径都有显式处理（无静默吞错）
- [ ] 没有为通过测试而弱化断言
- [ ] 文档已更新（若改动影响架构 / 约定）
- [ ] 重大决策已记录为 ADR（[`docs/decisions/`](docs/decisions/)）
- [ ] 提交信息符合 Conventional Commits

## 5. 报告 Bug / 提建议

- 用 [`BUG / FEATURE 模板`](.github/ISSUE_TEMPLATE/) 开 Issue。
- **Bug 必须包含可复现步骤**——这呼应我们的"不静默吞错"原则：程序应已暴露足够细节供你填写。

## 6. 行为准则

保持尊重、建设性。对事不对人。AI 与人类一视同仁。
