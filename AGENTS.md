# AGENTS.md

> 本文件是**所有贡献者（人类与 AI agent）**的统一协作守则。
> 开工前必读本文件与 [`docs/principles.md`](docs/principles.md)。
> 所有的注释和文档都必须用中文撰写，专业名词、和代码里面名字对应的概念必须保持一致并用英文，以防止描述的歧义

---

## 项目是什么

**股票模拟游戏，Web 优先**。三阶段演进：

1. **Stage 1** — 纯前端单机可玩（本地存档，无后端）
2. **Stage 2** — 前后端分离，后端**可选**（联机 / 持久化 / 远程部署）
3. **Stage 3** — Tauri 桌面应用

游戏**核心逻辑**（市场模拟、撮合、组合计算）必须与渲染层解耦，可独立测试。
当前完成度与未覆盖边界见 [`docs/roadmap.md`](docs/roadmap.md) 和
[`docs/trading-rules.md`](docs/trading-rules.md)。

## 三条铁律

> 这是项目宪章。违反任何一条，PR 不予合并。

### 🧪 1. 测试驱动（TDD）
先写失败的测试，再写实现，再重构。一段代码"难以测试"= 设计有问题。详见
[`docs/testing.md`](docs/testing.md)。

### 🛡️ 2. 防御式编程 / 不静默吞错
- 禁止：空 `catch`、`catch { return null }`、用 `?? 默认值` 掩盖异常。
- 任何错误都要**显式展示给用户**：发生了什么、在哪、为什么、怎么反馈。
- 宁可显式崩溃 + 详情，也不要悄悄 fallback 产生不可解释行为。
- 详见 [`docs/error-handling.md`](docs/error-handling.md)。

### 📐 3. 最小惊讶 + 诚实
代码只做它承诺的事；汇报工作要诚实（失败就说失败，跳过就说跳过）。

## 大 A 语义与独立复核门禁

本项目所有改动都以**现行中国大陆 A 股（沪深市场）语义**为领域基线。即使改动看似只是
重构、类型调整、UI 文案、存档、API 或工具链，也必须确认没有改变或模糊交易语义。

- 涉及交易制度时，先查交易所、中国结算等官方现行规则，记录依据与适用日期；不得凭印象实现。
- 不同交易所、板块或证券类别规则不同时，必须显式建模差异；不得静默取平均值或用一个默认值冒充全部规则。
- 游戏尚未实现的真实规则必须明确标为“简化”或“不支持”，并在 [`docs/trading-rules.md`](docs/trading-rules.md) 登记。
- 代码、类型、测试、UI 文案、存档/API 契约与文档必须使用一致的 A 股概念和单位。
- 每批改动完成后，必须由一名**未实施该改动的 subagent** 独立审查完整 diff，回答：
  1. 是否符合大 A 语义，依据是否可靠；
  2. 改动是否为需求所必需、是否保持最小范围；
  3. 是否存在遗漏的边界测试、跨层语义漂移或不必要复杂度。
- subagent 的有效发现必须修复并再次复核；无法修复时必须明确报告。未经这一步，不得宣称改动完成。

## 开工 checklist

- [ ] 读 [`docs/decisions/`](docs/decisions/) — 相关决策是否已敲定？
- [ ] 读 [`docs/open-questions.md`](docs/open-questions.md) — 你的改动是否触及开放问题？
- [ ] 确认改动落在正确的层（engine / web / server / desktop），遵守依赖方向（[`docs/architecture.md`](docs/architecture.md)）
- [ ] 新增依赖？→ PR 里说明理由；核心依赖需先有 ADR
- [ ] 需要代用户处理 Git 初始化或日常 Git 流程？→ 读 [`docs/git/AGENTS.md`](docs/git/AGENTS.md)
- [ ] 确认本次改动符合大 A 语义；完成后安排独立 subagent 审查语义与必要性

## Agent 工作文件目录

- Agent 任务产生的工作文件（计划、审计台账、排查笔记、交接及复核记录等）统一放在仓库根目录的 `agents/<work-topic>/` 下，按工作主题归档；不要放入 `docs/` 或散落在根目录。
- 例如：本轮历史实现审计放在 `agents/implementation-audit/`。同一主题的后续工作文件复用该目录。
- `docs/` 保留正式的产品、架构、领域规则及使用文档；任务结论需要成为正式规范时，再同步到对应文档，不用工作记录替代 ADR 或正式规则。
- 移动工作文件时同步修正引用和相对链接，保留既有内容与历史证据，不顺带迁移无关文件。

## 测试与资源利用

- 根据任务需要选择验证方式和并发度，记录实际命令、结果与重要限制。
- 现有测试 runner 可能内置超时或失败即停止的行为；使用前确认其实际行为，不能把未完成的验证报告为通过。


## 提交规范（Conventional Commits）

```
<type>(<scope>): <subject>

type:  feat | fix | docs | test | refactor | chore | perf | build | ci
scope: engine | web | server | desktop | docs | test
```

示例：
- `test(engine): 为价格波动模型增加边界用例`
- `feat(web): 接入股票列表视图`
- `docs(architecture): 说明 engine/web 分层`

> 详细流程（分支策略、PR 流程、CI 要求）见 [`CONTRIBUTING.md`](CONTRIBUTING.md)。

## GitHub 操作一律用 `gh` CLI

本项目所有 GitHub 操作（建仓库、PR、Issue、Actions、Release）**统一用 `gh`**（本机已装 2.95）。
不用网页、不用手动 `git remote`、不用其他封装工具。推送/建公开仓库/发版属对外不可逆操作，执行前须人类确认。

## 绝对不要

- ❌ 为让测试通过而篡改 / 删除 / 弱化断言
- ❌ 静默 `catch` 或用默认值掩盖异常
- ❌ 未读 ADR 就擅自定技术路线
- ❌ 一个提交混入多个无关改动
- ❌ 编造 API / 文件 / 测试结果
- ❌ 未经确认就 push / 建公开仓库 / 发版
- ❌ 用非 `gh` 的方式做 GitHub 操作

## 文档导航

| 主题 | 文档 |
|------|------|
| 工程原则 | [`docs/principles.md`](docs/principles.md) |
| TDD | [`docs/testing.md`](docs/testing.md) |
| 错误处理 | [`docs/error-handling.md`](docs/error-handling.md) |
| 架构 | [`docs/architecture.md`](docs/architecture.md) |
| 技术栈 | [`docs/tech-stack.md`](docs/tech-stack.md) |
| 决策记录 | [`docs/decisions/`](docs/decisions/) |
| 待定问题 | [`docs/open-questions.md`](docs/open-questions.md) |
| Git 工作流指令（自动代办） | [`docs/git/AGENTS.md`](docs/git/AGENTS.md) |
