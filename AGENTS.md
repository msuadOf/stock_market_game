# AGENTS.md

> 本文件面向**所有贡献者（人类 & AI agent）**，是 [`CLAUDE.md`](CLAUDE.md) 的人类视角版本。
> 两者内容一致，措辞不同。开工前**必读其一** + [`docs/principles.md`](docs/principles.md)。

---

## 项目是什么

**股票模拟游戏，Web 优先**。三阶段演进：

1. **Stage 1** — 纯前端单机可玩（本地存档，无后端）
2. **Stage 2** — 前后端分离，后端**可选**（联机 / 持久化 / 远程部署）
3. **Stage 3** — Tauri 桌面应用

游戏**核心逻辑**（市场模拟、撮合、组合计算）必须与渲染层解耦，可独立测试。

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

## 开工 checklist

- [ ] 读 [`docs/decisions/`](docs/decisions/) — 相关决策是否已敲定？
- [ ] 读 [`docs/open-questions.md`](docs/open-questions.md) — 你的改动是否触及开放问题？
- [ ] 确认改动落在正确的层（engine / web / server / desktop），遵守依赖方向（[`docs/architecture.md`](docs/architecture.md)）
- [ ] 新增依赖？→ PR 里说明理由；核心依赖需先有 ADR


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

## 绝对不要

- ❌ 为让测试通过而篡改 / 删除 / 弱化断言
- ❌ 静默 `catch` 或用默认值掩盖异常
- ❌ 未读 ADR 就擅自定技术路线
- ❌ 一个提交混入多个无关改动
- ❌ 编造 API / 文件 / 测试结果
- ❌ 未经确认就 push / 建公开仓库 / 发版

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
