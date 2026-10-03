# 基础配置、日历与计算接缝复核（2026-10-03）

## 基线与方法

- 当前基线 `8cf34a1ce2d893f003e1d4c34d7c2bea170dd4cb`；对比 `b89afb3`。在独立工作树 `.worktree/implementation-reaudit` 核对，未带入主工作区未提交文件。
- 全文阅读 `docs/simulation-calendar.md`、`docs/decisions/0008-gpu-and-compute-offload.md`、`docs/superpowers/specs/2026-06-29-initial-positions-design.md`，并承接上轮 R12/R18 原文覆盖记录；另外读取当前架构、开放问题及 OOP 实施摘要/最终复核，历史复核结论不代替本轮代码证据。
- 本轮检查当前调用方、实现与区间差异，不重写历史规范、不重新联网核验制度；未运行游戏测试、编译、浏览器或性能验收。

## 逐项判定

| ID | 状态 | 当前代码证据 |
|---|---|---|
| G15 | 仍缺 | `packages/engine/src/calendar/holidays.rs:72` 先排周末，找到当年官方覆盖后仅在 `entry.covers(date)` 时返回休市；非覆盖日期仍执行 `simulated_holiday_kind`（88 行）。该文件区间内未改，官方覆盖不能取消模拟休市的边界仍在。默认无官方覆盖，不声称默认局已触发。 |
| G17 | 仍缺 | `packages/engine/src/indicators.rs:237` 单项顺序计算，251 行批量方法使用 Rayon；全仓调用搜索后读取三宿主真实入口：`apps/web-wasm/src/lib.rs:435`、`apps/server/src/routes.rs:632`、`apps/desktop/src-tauri/src/lib.rs:60` 均调单项方法。批量方法仍仅测试调用。本次 EMA/KDJ owner 提取未增加生产批量接线；指标功能本身已有，不把它整体重开。 |
| G29 | 仍缺 | `packages/engine/src/session.rs:1090` 校验 ByKind，1098 行正流通盘时只累加存在类别的权重；零 NPC 会在 1108 行拒绝。`seed_float` 的空 NPC 早退在 1658–1660 行，但新局先校验，因此不能核销零 NPC 边界。区间变化只是 state owner 访问迁移。 |
| Q01 | 仍待定 | `packages/engine/src/money.rs:40` 仍以 transparent i64 序列化；`apps/web/src/host/protocol/wire-values.ts:48` 仍要求 signedSafeInteger。两文件相关编码未改，不自行扩大 Web 数值范围或移除守卫。 |
| Q03 | 仍待定，政策保护存在 | `packages/engine/src/session.rs:957` 拒绝非当前 simulation_policy_id；`packages/engine/src/session/persistence.rs:260` 恢复再次校验。政策 ID 与完整冻结规则集合的文档映射仍应澄清，不能因没有同名 RegulationProfile 类型判功能缺失。 |
| Q06 | 仍待定 | `packages/engine/src/session.rs:1090` 仍允许有效正权重归一，不要求总和约等于 1；1739 行起 Retail eligibility 抽样，1753 行起按类别选择 Pareto 参数。旧 spec 与现行分配差异未由本次重构改动或新增决定核销；不能擅自回退算法。 |
| Q09 | 仍待定 | `packages/engine/src/compute.rs:59` CpuBackend 是纯批量接缝，107 行工厂的 Gpu 仍显式不可用；生产 session 的个体认识/并行决策另有路径，未调用工厂。该文件区间内未改。保留接缝不等于生产运行时可切 GPU；positions Vec 与当前账户/候选协议的关系也未被 OOP 决定替代。 |

## 指标重构与已实现能力

读取 `indicators.rs` 的区间差异：EmaSmoother 保存上一值及两个系数，KdjAccumulator 保存 K/D 和输出序列；原 12/26/9 系数、首次采样初始化、九样本窗口及运算次序仍保留。新增保护测试只是本轮读取的源码，未执行，不用于宣称性能或全量数值验收。图表 Rust MACD/KDJ 功能与 G17 的批量接线是不同层次。

## 边界

本报告不把模拟节日表说成真实行情数据，不要求恢复真实市场校准；未新增交易规则、自动补钱、日内持久存档、强卖或任意请求配额。以上全部是当前源码静态判断，不是正常游戏运行失败复现。
