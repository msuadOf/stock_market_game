# OOP domain implementation EOF 独立复核

- 范围：`operations-result.md`（43 行）、`operations-review.md`（52 行）、`orderbook-result.md`（110 行），逐文件读至 EOF；产品基线按任务指定 `08e4fc7`（merge 同）。
- 结论：未发现可证实的新增产品缺陷或可直接成立的遗漏；有一个边界候选经正式契约和实际 owner/caller 反证，不升级为发现。下面是对文档旧结论的逐章核销，不替代产品编译/测试验收。
- 需求依据：复核记录指向 `agents/oop-refactor-audit/challenge-2026-10-03/action-index.md`，但该路径在本 worktree 不存在；因此不伪称读过该动作正文。以仓库正式 ADR/领域文档优先，辅以结果及 reviewer 完整记录中的动作摘要。operations 的正式相关边界为 ADR-0019/0025、`docs/principles.md`；订单簿/账户既有交易语义以 `docs/trading-rules.md`、ADR-0017/0019/0025 为准。

## 逐章扫描矩阵

| 文件/章（原行） | 对照的 owner / current caller / 后续修复 | 复核旧结论与新证据 |
|---|---|---|
| operations-result: 标题及状态/基线（1–8） | 复核绑定见 `operations-review.md:1–22`；当前源码位于 `company/operations/` | “三个产品文件、静态复核、指定短测由 root 汇总”的范围描述与后续 reviewer 记录一致。该 worktree 无动作索引原文，不能独立确认动作动机或声称重新运行测试；保留原验证局限，不扩大为运行验收。 |
| operations-result: N19（10–17） | `IndustryPairView` 定义于 `config.rs:97`；build caller `core.rs:192`；日流 caller `day.rs:308`；四行业 delegate 从 `day.rs:236` 起。错误类型为 `KindFlowMismatch` | 强类型临时借用仍配对各自 books/params。`at_build_guard` 额外校验 `spec.kind`，`at_existing_guard` 只匹配 books/params；没有把构造时 guard 偷渡到每日恢复 DTO。review 所称守卫时点及公开 DTO/serde 接受集保持有源码支持。 |
| operations-result: N20（19–28） | `OperatingDayRun` 在 `day.rs:33` 借用原 `CompanyOperations`；公开 caller `advance_civil_day`；真实宿主 caller `session/company_operations.rs:84`；历史重建 caller `operations/history.rs:54`；到期 owner `dispatch.rs:16` | 日期、报告临时向量归本次 run；scheduler、账套、RNG 与日期权威仍在原 owner。`advance` 顺序可直接见 `day.rs:79`。自然日而非证券交易日的表述不改 A 股 T+1/交易日语义；不把公司内部现金并入投资者账户。 |
| operations-result: 统一验证交接（30–43） | 七个测试位于 `day.rs` 测试模块；原集成入口列于结果章 | 报告清楚区分了 root 的指定短测通过、完整回归未跑和普通测试 deadline；不把建议 filter 当作执行记录。文中 Err 前写入保持及未事务化与正式“最小惊讶/显式错误”原则相容，前提是调用方不误认为 Err 等于回滚；review 亦明确未作回滚承诺。 |
| operations-review: 标题、范围/权威输入（1–9） | 三个完整产品文件的范围与 operations-result 相符；dispatch/injections 是被核对的既有 owner 接缝 | Reviewer 说明未参与实现、列出静态读取范围和未联网复核边界。动作索引缺失导致本次无法独立核对其“权威需求正文”，但不能据此推翻有源码/ADR佐证的行为事实。 |
| operations-review: 版本绑定/结论（10–24） | SHA 绑定所列 `config.rs`、`core.rs`、`day.rs`；当前实现结构与报告所述一致 | SHA 可作为旧审查时的版本证据，不将旧指纹当作当前哈希实测。结论限定为静态审查“未发现需修复问题”，没有声称编译通过；描述准确。 |
| operations-review: 大 A 语义（26–31） | 自然日循环 `day.rs`；经营资金由各 IndustryBooks 维护；投资者账户在 Account/Session | 核验结论属于重构保持原语义，而非官方交易规则新解释。`docs/company-accounting.md` 和 ADR-0024 提供资金隔离/游戏经营简化背景；review 已承认未联网重验规则，旧结论没有冒充官方规则认证。 |
| operations-review: 必要性/最小范围（33–38） | N19/N20 两个 owner 与实际 caller 如上；到期仍由 `CompanyOperations::dispatch_due_on` | 没发现平行状态、重复成熟动作 owner 或无依据 trait/API。对“头注同步顺序是必要说明”的判断合理且范围极小。 |
| operations-review: 边界、跨层、复杂度（40–49） | 日期守卫 `day.rs:43–65`；采样/排序 `day.rs:79–149`；到期派发 `dispatch.rs:16`；flow 错误在 `day.rs:154+` 上抛 | cache 失效先于日期校验、抽样分批激活、派发/flow 中途错误留下先前写入均可由源码复现。review 对未专测日期上界、submit 中途失败及 dispatch 中途失败明确标为未新增专测，而非声称覆盖。未找到需要将既有部分写入静默改事务的正式契约。 |
| operations-review: 验证限制交接（50–52） | root 最终运行记录另在 final-summary；本复核未运行命令 | 旧 reviewer 把待执行完整回归留给 root，符合记录边界；本次只读扫描不把结果改写成通过。 |
| orderbook-result: 标题/逐动作（1–15） | Account `account.rs:95+`；Order `orderbook.rs:206`；Market `market.rs:259,265`；BookState `book_state.rs:10`；treap wrapper `filled_orders.rs`/`resting_index.rs` | 五个动作摘要可由 owner 和实际 caller 支撑。特别是 N02 顺序：taker 路径 `orderbook.rs:431–438`，maker 路径 `book_state.rs:134–149`；N37 不拿 next_seq 归 BookState；N38 不混淆 identity treap priority 与成交 price-time 优先级。 |
| orderbook-result: 文件范围（17–26） | 所列文件存在；另核对 session/pipeline/host caller | 文件清单是主体 owner 范围，文中明确跨文件 caller 由其他任务组迁移。未把 caller 变更漏报成 owner 内修改。 |
| orderbook-result: 冻结接口（28–48） | Account getter、有限恢复入口；Market crate 内恢复/竞价写口；integration tests 及 serde DTO | 私有化 Rust 字段/setter 的源码兼容影响有诚实说明；serde DTO/wire 字段不等于 Rust public API。恢复入口有 `session.rs:2725,2735,2762` 实际使用，策略入口另有 `persistence/v2.rs:436`、`npc_state_projection.rs:206`；价格竞价入口由 `auction_day_end.rs:1424` 调用。没有从证据推出仓库外兼容保证。 |
| orderbook-result: 行为保护与验证（50–77） | `state_contract_tests.rs`、Account/OrderBook 测试；后续 `orderbook-review.md` F1 修复 | 记录先描述未由 worker 执行 Cargo，再列 root 指定短测通过。F1 taker Money overflow 缺测已有新测试并在独立复审关闭；修复细节见 reviewer 的“有效发现与修复复核”。没有删弱断言。 |
| orderbook-result: Market fixture 迁移（79–103） | 3 个完整 case 在 `market.rs::price_limit_state_tests`；helpers 留在内部测试模块；原 integration case 删除 | reviewer 对旧/新正文、helper 以及原十例分布作静态完整比对：三项搬移、七项留下，各一次；要求 runner 包含新 lib filter 的限制明确。不能把搬移本身说成新增覆盖，也不能只运行 integration 后声称十项全覆盖。 |
| orderbook-result: 领域语义/复核（105–110） | `docs/trading-rules.md`、ADR-0017/0019/0025；完整复核 `orderbook-review.md` | 没有新增交易制度。price-time 顺序、maker price、分/股单位、T+1、费用及持久化边界由正式契约承接；文档明确 treap priority 不是成交顺序、局部 BookState 不是整 tick 原子回滚。静态独立复核不等同最新外部规则查证。 |

## 原文代码与失败边界

与本次 EOF 扫描直接相关的原文（`packages/engine/src/company/operations/day.rs`）：

```rust
fn advance(mut self) -> Result<CompanyDayReport, OperationsError> {
    self.expire_shocks();
    self.sample_and_activate_shocks()?;
    self.dispatch_due_actions()?;
    self.advance_company_flows()?;
    self.schedule_next_interest()?;
    Ok(self.build_day_report())
}
```

以及 `orderbook.rs` 的成交顺序原文：

```rust
let maker_filled_value_after = maker.filled_value_after(fill_value)?;
order.filled_value = order.filled_value_after(fill_value)?;
order.filled_qty = order.filled_qty_after(fill_qty)?;
order.qty -= fill_qty;
self.state
    .apply_maker_fill(side, &maker, fill_qty, maker_filled_value_after)?;
```

代码和文档都表明这些 `Result::Err` 不是一般事务 rollback 契约。候选问题“经营日或盘口局部动作失败会留下前序写入”不构成此批新回归：这是实现前已有边界，结果/复核均显式记录；ADR-0017 的整 tick 候选丢弃与 `commit_tick` 失败隔离也不能错误套用到独立 `CompanyOperations` 经营推进或 OrderBook 局部 `apply_changes`。该差异应继续清楚标注，不能自动免责任何原有缺口；本轮没有发现产品行为新变更造成边界恶化的证据。

## 新候选与反证

1. **候选：crate 内恢复入口可能被绕开完整存档校验。** `restore_balances`/`restore_prices` 是 `pub(crate)`，方法本身不验证输入。反证：正式契约 ADR-0019 §2 要求存档事实可编辑、派生数据读取时重建；源码调用追踪见 `session.rs` 的 `validate_save_slot` 后恢复及 `Market` caller，reviewer 也核对过该顺序。策略替换通过 `AccountBook::get_mut` 维持缓存失效。扫描到的调用均有明确 owner/校验或投影语境，未发现实际绕过；因此是边界提醒，不是有效缺陷。未来增加 caller 时仍须守住前置条件。
2. **候选：运行“全部短测通过”可否代表 reviewer 实测通过？** 反证：三个报告均区分 worker 未运行与 root 集中验证；operations-review 明确静态复核不是编译/测试通过，orderbook-review 对新增 F1 用例也明确只做静态核验。没有把未执行证据冒充执行结果。
3. **候选：partial write 与“大 A 语义”冲突？** 反证：该处是内部模拟经营/局部撮合失败语义，不是 A 股交易规则；正式 ADR-0017 仅要求市场 tick 事务边界，不扩张到所有 engine API。项目错误原则反对静默 fallback，但不能据此凭空许诺跨多个 owner 的回滚。对既有缺口仍需按其正式契约报告。

## 最终核销

- 未发现需要修复并再次复核的有效发现；独立扫描保持只读产品代码。
- 产品代码中的原有 Err 部分副作用未以“旧行为”自动判定无风险；这里只确认没有证据表明这些实现记录对其作了隐瞒或把局部边界谎称为整批原子性。
- 未运行测试、编译、Git 写命令或长任务。该文件是本次唯一新增内容。
