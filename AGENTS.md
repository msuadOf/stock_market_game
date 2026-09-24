# AGENTS.md

> 本文件是**所有贡献者（人类与 AI agent）**的统一协作守则。
> 开工前必读本文件与 [`docs/principles.md`](docs/principles.md)。

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

## 多核测试与资源利用

- 普通自动化测试的单条命令和单个 case **均以 10 秒为硬上限**；超过时必须缩小 fixture、拆分独立用例或消除重复工作，不得把分钟级运行伪装成普通单测。
- 适用的 Node 普通测试必须同时配置 10000ms case timeout 和整命令进程树 deadline；完整回归必须显式分类为长验收并使用共享 300000ms deadline。
- 只有无法由代表性短 fixture 替代的验证才可列为长测试。长测试的每个 child 进程硬超时不得超过 **5 分钟（300000ms）**，从批次启动到进程树终止、临时文件清理完成的共享 wall-clock deadline 也不得超过 **5 分钟**。K7 在其中固定预留最后 1000ms 仅供终止和清理，执行、校验与 manifest 发布必须在前 299000ms 内结束；正式命令还须由进程外 deadline supervisor 执行，避免被测 Node 事件循环阻塞自身计时器。禁止用环境变量把上限放宽到 2 小时、6 小时或更久。
- 测试、编译和验证必须显式使用机器的多核能力；禁止让可并行的长任务长期只占用单个 CPU 核心。
- 测试框架默认并发不足，或单个长用例使其余核心空闲时，必须在保持测试隔离和结果确定性的前提下，按测试二进制、套件或互不冲突的分片并发执行。
- “全量测试同时最多一个”表示只允许一个完整验收批次在途；该批次内部仍必须多线程或多进程并行，不能据此退化为单核串跑。
- 启动长任务前应记录实际并发参数；运行中核对进程树的 CPU/线程利用率。发现意外单核长跑时，应安全停止、保留日志，修正并发方式后重跑。
- 只有测试语义、共享外部资源或工具硬限制确实禁止并行时才可串行；必须先说明具体原因、隔离该用例，并让其余可并行工作继续，不能把串行作为默认方案。


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
