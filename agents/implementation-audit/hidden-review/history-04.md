# 历史来源补扫 04

## 范围与读取完整性

按 `history-plan.json` 的 sources 下标 9–11 审阅，原文均从 revision `c139a69d2a09220e349912376d4be83d8859795f` 通过 `git show revision:path` 连续读取至 EOF；读取长度与计划一致，无截断：

| 下标 | 来源 | 行数 | SHA-256 |
|---|---|---:|---|
| 9 | `.omo/notepads/escrow-parallel-engine/todo-2a-protocol-resolution.md` | 7 | `ca81240befe30ce2837d167aeabfdbc54075b4a999a2381ddea53884a536acad` |
| 10 | `.omo/notepads/resolve-blockers-wayland/decisions.md` | 16 | `e79824b56d460a6f0fc3d7cdf9c54ac4e130a9b8c949584dc49a987563d98f4e` |
| 11 | `.omo/notepads/resolve-blockers-wayland/issues.md` | 221 | `39e26ed9a168d02ddf770e7c3d4c6fee4a776dffb4e0997117a3a9a78f1dd141` |

## 逐项裁定

### 下标 9：Todo 2A 协议澄清

历史 canonical-order blocker 已由用户裁定解除：`TickFrame` 按 tick 保序，帧内事实按多重集理解，`seq` 用于覆盖、去重与重连游标；不得把旧事件数组相对顺序当业务因果，也不得跨 tick 重排或丢弃时序点。记录还明确 Todo 2B 尚未开始，早期 attempt 不能替代 collector verify。

较新的 ADR-0017 明确将旧 `npc → player → plan_chain` 类序和全局密封序的交易优先语义交由 ADR-0018 的真实资源冲突/依赖顺序决定；ADR-0018 描述逐 tick 接收及已提交观察版本。43b1aa5 的 Web protocol parser/normalizer 与 frame coverage consumer 仍以 `seq` 做连续性、规范化及去重，并保留 TickFrame 边界。因此历史阻塞已退役，协议约束已落在当前消费者；不把旧顺序恢复为候选，也不把这份记录中“2B未开始”误作现状。

### 下标 10：Task 7 跨年父计划裁定

裁定保留 `IncompatibleExecutionState` 严格守卫；只有终止计划且没有活动子计划时，才删除仍关联且归该计划所有的父记录，再写入终态。独立复核认定不改变 A 股委托、T+1、批次、集合竞价、交易日历、会计或披露语义。

43b1aa5 的 `packages/engine/src/session/plan_execution.rs` 与 `plan_execution/actions.rs` 保留不兼容状态错误守卫；`session/execution.rs` 有 `unlinked_parent_reverse_rebuilds_execution_from_the_new_target` 覆盖解除关联后的重建。该历史例外有现行实现及消费者证据，分类为已实现；不是需要放宽全局状态校验的新候选。

### 下标 11：Wayland blocker 历史问题记录

记录涵盖 runner缺失后撤回、Worker自身存档往返失败、默认 Server body 限额、host-parity 覆盖缺口、运行时版本问题、原始矩阵长运行状态、进程清理/来源身份/可视验证等多个当时的失败、限制与待办。它们是截至 2026-09-17 的过程台账，不能按标题或未完成复选框推断当前产品缺陷。

对照 43b1aa5 真实 consumer：Server 的 `MAX_LOAD_BODY_BYTES` 已按引擎存档解码上限加封装余量设置；Worker、Server、Tauri 的 host-parity 接口与测试存在；当前模拟验收 runner 对来源指纹、子进程期限、清理和 manifest 发布设有实际控制。旧记录中 `host-parity` 与 K7 的未执行/不完整声明属于历史证据边界，不等同本轮必须补做的产品需求。Resolution 总账已覆盖容量、工具期限、进程监督和宿主覆盖等重复议题，本分片没有从旧记载中新增独立缺口。

## 候选与状态

无新增候选。下标 9：已裁定并由当前协议消费者承接；下标 10：已实现且严格守卫仍在；下标 11：历史失败与验收/证据限制，相关当前边界已由较新决策和源码更新，未从旧状态推导新 G。无新的 A 股规则主张；本轮裁定仅涉及协议游标、计划父子生命周期和验证证据。

本结论只核验指定三份来源及其可确认的现行关联，不宣称运行任何测试或完整矩阵。
