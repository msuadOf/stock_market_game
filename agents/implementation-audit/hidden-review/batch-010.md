# 批次 010：Session-N10 独立复核

基线：`43b1aa5f25226c72976ca172d32f4a8eeb2272ad`（`.worktree/implementation-reaudit`）。仅复核历史挑战条目 `session-R2-N10`：LiveOrderReservations 快照预留汇总；未改产品代码、未运行测试/回归、未写 Git。

## 输入完整性

| 源文件（项目根相对路径） | SHA-256 | 行数 | 标题/章节矩阵 | EOF |
|---|---|---:|---|---|
| `agents/oop-refactor-audit/challenge-2026-10-03/session/report.md` | `025499411a587e254ac5730cd8a46b4062a72611efc7d65c73add71887c19969` | 969 | `# Session / strategy 第二轮 OOP 挑战审计`；§已有动作去重与反证；§新动作与增强；N01–N12、E01；§全部盲读候选处置表；§完整覆盖与证据 | 已依 `1–500`、`501–969` 连续分段读取至行969 |
| `agents/oop-refactor-audit/challenge-2026-10-03/session/review-final.md` | `3e8df6a34f6802c0badcf0e570121d59a41ae78f711d7d7da8f19c5702676b10` | 67 | `# Session / strategy 最终独立复核`；§最终受审版本与全文读取；§三门结论；§本轮发现与修复复核；§原复核的证据边界；§独立源码全文读取；§机械与材料覆盖边界 | 已连续读取至行67 |
| `agents/oop-refactor-audit/challenge-2026-10-03/session/review.md` | `92472e3ccb2090e34d301e12eb46798a0c3f7c64e4239a4fe1f947136cfd2895` | 28 | `# Session / strategy 第二轮 OOP 挑战独立复核`；§三门结论；§覆盖与修复复核 | 已连续读取至行28 |

章节矩阵完整列出 `report.md` 锚点：N01注意力候选堆、N02开局账套、N03计划生命周期、N04个人状态、N05机构风险、N06存档校验上下文、N07累计收费审计、N08异步root、N09批次续作、N10快照预留、N11目标权重到数量、N12散户决策上下文、E01发布事实游标。N10 段在 665–727 行；表格亦将原候选 `15-1` 映射到 N10。三个复核文档头部记录为 GFM `#`，其余层级分别为上述 `##` 与 N01–N12/E01 的 `###`。

## N10 核对矩阵

| 判定 | 当前 caller → owner → consumer / 反证 |
|---|---|
| 已实现（不是当前缺口） | `packages/engine/src/session/snapshot.rs:63-108` 已定义临时 `LiveOrderReservations`、`record_order` 与 `take_for`；`:131-157` 的 `snapshot_inner` 用同一实例汇总连续簿与集合竞价簿；`:182-204` 消费到 `AccountSnap`。owner 生命周期限于一次快照，权威订单仍由 Session 持有。 |
| 需求必要性/最小范围 | N10 指定的类型和两个账本正是生产实现。无需再做第二次同名抽取或加持久化 owner。`AccountSnap` 的公开 JSON/TS 仍仅有既有 `reserved_cash`、`reserved_sell_qty` 字段（`:32-41`）；本项不产生新存档权威事实。 |
| 边界测试反证 | 当前目标树 `snapshot.rs:236-301` 用单测试实际组合连续买卖单、部分成交、跨证券卖单及竞价买卖单，断言总 `reserved_cash` 与两证券 `reserved_sell_qty`；`:303-342` 覆盖账户隔离和空账户零值；`:344-382` 覆盖卖股数与现金累计溢出的显式 panic。故挑战报告当时所述旧测试覆盖不足（只检 reserved_cash/positions.qty）准确描述其引用的 `continuous_tick_transaction_tests.rs`，但不能推导当前 N10 测试仍缺：后续快照 owner 测试已覆盖。这里仅静态读测试源码，未运行。 |
| 计算/错误路径 | 卖单剩余股数以 `checked_add` 累加（`:83-93`）；现金预留先经 `live_cash_reservation`，再用 `Money::add` 检查（`:94-100`）。缺少订单时 `take_for` 返回零值/空 map（`:102-106`），且账户无订单本身是合法状态，不构成吞错。 |
| 交易语义 | 本对象仅投影在簿剩余委托的游戏冻结资源，不是交易所清算或中国结算规则。continuous 传入订单剩余 `qty` 和累计 `filled_value`（`:133-142`），auction 传入剩余 `qty`、limit 与零已成交额（`:145-155`）；不代替可卖股份、T+1 或受理校验。遵守 `docs/trading-rules.md` 的金额分、数量股和真实委托边界，不据此主张新的 A 股制度。 |
| G01–G68 / Q 对照 | 现行总账明确列出现行产品缺口 G01–G68（`implementation-audit-2026-10-02.md:27`），其中没有 snapshot reservation 相关 G 项；文档的 `R10` 旧 Session 快照承诺为已实现（`:255`）。故 N10 不新增现行缺口，不核销无关 G 项。Q7 将公共存档限定为日终权威 SaveSlot；Q12 允许交易费用退出投资者资金池、不因现金减少而补钱。N10 仅快照读模型，不改上述决策。 |
| 待定/文漂/历史证据 | 未发现 N10 对应待定契约。N10 报告中的“若实施”是挑战审计对可选候选的建议，不等于待实现承诺；其旧测试描述应作为当时引用测试覆盖范围保留，后续新 `snapshot.rs` 测试是当前实现反证。 |

## 结论

session-R2-N10 所述 owner 已存在且被生产路径使用。独立复核结论：**已实现；非现行缺口，无需另做重构。** 当前快照测试源码已补足报告指出的 reservation sell-side / continuous+auction 组合覆盖。未运行这些测试，不声称运行通过。审查范围只含 N10 与其相邻规则/总账；不据此对 N01–N09、N11–N12、E01 或 G01–G68 其他项目背书。
