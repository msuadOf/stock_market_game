# 代码命名与计划编号

变量、函数、类型、模块和测试名描述业务职责、数据内容或验证行为。新名称不使用
AI 计划中的 `p1`、`b1`、`K7`、`task9` 等编号；局部对照值也说明各自角色。
需求追溯使用具体规范或 ADR 链接，不把一次实施任务号当成领域类型。

## 当前示例

| 原名 | 当前名称 | 含义 |
|---|---|---|
| `p3_validation` | `account_validation` | 账户资源与委托校验 |
| `P2CandidateKey` | `IntentCandidateKey` | 待处理委托候选身份 |
| `P3ValidatorDriver` | `AccountValidatorDriver` | 增量账户校验驱动 |
| `PreparedB1ContinuousTick` | `PreparedContinuousTick` | 已完成校验、待提交的连续交易 tick |
| `prepare_b2_auction_tick` | `prepare_auction_tick` | 集合竞价 tick 的准备 |
| `K7_CHILD_TIMEOUT_MS` | `SIMULATION_CHILD_TIMEOUT_MS` | 模拟验收子进程时限 |
| `runTask9Matrix` | `runEscrowVerificationMatrix` | 托管资源验证矩阵 |
| `p0Released` / `p1Live` | `expiryReleased` / `allocationLive` | 到期释放与分配截点存量 |
| 地产测试 `p1()` | `project_id()` | 测试项目身份 |
| 日历测试 `v1` / `v2` | `saved_policy` / `future_default_policy` | 冻结政策与未来默认政策 |

会计科目表、日历算法、存档、协议、报告修订与第三方 API 的真实版本号继续保留，
例如 `ReceiptSourceV2`、`runtime_v2`、`industrial_chart_v2`、UUID `new_v4`。
ECL 的 `Stage1/Stage2/Stage3` 是财务减值阶段，不是实施步骤。

## 序列化与历史证据

此次改名不迁移存档或改变 JSON 形状。Rust 职责名称通过显式 `serde(rename)` 保留已声明的
序列化拼写：`QuoteExpiry` 的来源标签仍为 `P0` / `P0Expiry`，
`CreatedAtValidation` 仍为 `P3Created`，`expiry_released` / `allocation_live`
对应原来的 `p0_released` / `p1_live`。
调度边界的七个旧标签也保持不变，与现行证据消费者一致。
事件来源枚举与收据来源枚举是不同契约；实际到期撤单的公开事件仍使用既定的
Account/Sealed 身份域，不能从新 Rust 变体名重新推断或改写事件来源。

`k7-*` 验收 schema、`fresh_current_k7_setup` 来源身份和 `P0`–`P9` 测量键是保留的格式契约；
历史 `.omo` 证据、计划记录与源码见证不重写。新代码中的变量名与类型名说明职责，
解析器仍核对原始证据的标签、内容哈希和身份，不能静默改写旧证据或伪称新验收通过。

当前矩阵工具的运行身份字段有意同步迁移：summary schema 从
`escrow-task9-matrix-summary-v1` 改为 `escrow-verification-matrix-summary-v1`，
matrix version 从 `task9-runtime-v1-matrix-v1` 改为 `escrow-runtime-matrix-v1`，
scenario 从 `task9-runtime-v1` 改为 `escrow-runtime-v1`。生产者与消费者使用相同新标签；
这不是历史密封证据或 `k7-*` 格式身份的迁移。

模块、局部变量和格式名称的调整不改变申报数量、现金/股份单位、T+1、费用、价格时间优先、
集合竞价、同批释放可见性或原子提交边界。基线见 [交易规则](trading-rules.md) 和
[ADR-0017](decisions/0017-escrow-parallel-tick.md)。
