# ADR-0004: 前端状态管理使用 Redux Toolkit

- **状态 (Status):** accepted（前端状态层职责不变；数据源从「WASM」扩展为「WASM Worker 或 WebSocket」，见 [ADR-0005](0005-unified-engine-three-deployments.md)）
- **日期 (Date):** 2026-06-28
- **决策者 (Deciders):** msuad

## 上下文 (Context)

前端（React）需要一个状态管理方案。架构上 engine（Rust→WASM）是状态转换的核心，
但 UI 层仍需管理：对 engine 的调用编排、UI 本地状态（选中、弹窗、表单）、
从 engine 返回的状态/事件的缓存与派生。

候选：Zustand（轻量）、useReducer+Context（零依赖）、Redux Toolkit（成熟）。

## 决策 (Decision)

**前端状态管理采用 Redux Toolkit (RTK)。**

理由（决策者取向）：
- 成熟、生态大、文档完善；可预测的单向数据流与"显式优于隐式"原则契合。
- 强类型支持好（TypeScript），适合本项目防御式编程风格。
- RTK Query 可覆盖后续"后端可选"阶段的数据获取与缓存需求。

## 备选方案 (Alternatives Considered)

- **A. Zustand（轻量）** — 样板少、性能好，适合"engine 是核心、UI 状态层应薄"的设计；但生态与可预测性权衡上，决策者更看重成熟度。
- **B. useReducer + Context（React 原生）** — 零依赖；但中大型应用 Context 重渲染需小心，扩展受限。
- 备选不选的理由均为取舍偏好，非技术否决。

## 后果 (Consequences)

- **正面：** 单向数据流、DevTools 时间旅行（利于调试与"诚实反馈"）、RTK Query 为 Stage 2 数据层铺路、生态成熟。
- **负面：** 比轻量方案样板略多；需保持"engine 是状态权威，Redux 仅编排 + 缓存 UI 视图"的边界，避免状态双写。
- **后续需要做的：**
  - 明确状态边界契约：engine（Rust）持有权威游戏状态；RTK store 存放 UI 状态 + engine 结果的序列化快照，不重复计算业务规则。
  - 派发一个意图 → 调 engine → 用返回结果更新 store + 渲染。

## 关联 (Related)

- 关联开放问题：[`open-questions.md`](../open-questions.md) Q5（已解决）
- 配套：[`architecture.md`](../architecture.md)（状态与数据流）、[`tech-stack.md`](../tech-stack.md)
