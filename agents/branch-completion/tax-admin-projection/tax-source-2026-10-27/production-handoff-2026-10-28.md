# 税务来源与 R4 生产接线交接

## 本片已经实现但未代表生产接通

唯一按证券入口：

```rust
TaxDayBoundaryFacts::prepare(
    registry: &ShareRegistry,
    books: &[CashDividendTaxBook],
    day: &ShareSettlementDay,
    sources: &ShareMarketSourceContext<'_>,
) -> Result<TaxDayBoundaryFacts, TaxBoundaryError>
```

返回单一受核REG候选、按`book.account()`排序的books、按event/account关联的完整普通capture归属。读取使用`registry()`、`books()`、`registrations()`；归属字段private，只有`event_id()`、`registered_on()`、`account()`、`stock()`、`after_book_operation()`、`settled_receipts()`、`lots()`。禁止以输入位置或ID前缀替代账户与来源身份。

REG前驱/完整自然日/日历/完成水位/最终确认全集的唯一核验仍在`ShareRegistry::prepare_settlement_day`，该批每证券调用一次。税owner只负责tax日/归属，不复制市场dispatcher。

## 尚缺的真实生产责任

R4唯一owner `sol_r4_continue_28` 已明确承接其真实civil prepare统一装配：借E2 `FullInstalled`实际收盘账户、个人trade confirmations和完成水位，构造`ShareSettlementDay`与`ShareMarketSourceContext`，调用上述入口，将同candidate的REG/book/归属纳入唯一R4事务。当前还没有可供引用的真实生产函数名，不以规划补造API。

混合税阶段由R4在同pending、同AdminTransactionId内捕真实working阶段before/after，只复制该税stage typed ops引用的local集合，原operation_index保持组内顺序。税叶唯一core保持：

```rust
inspect_tax_stage(
    id: AdminTransactionId,
    stage_index: Option<u32>,
    groups: &[TaxStageGroup<'_>],
) -> Result<PreparedTaxProjection, TaxProjectionError>
```

`TaxStageGroup`借group_index/group、原`(operation_index,&AdminOperation)`与完整`(local,&before,&after)`；不能把事务初始cash0与最终90冒为税阶段before/after。必须有真实A信用100→B扣税10（100→90）→C非税资产变化；只有唯一最终commit成功后才发布受控tax receipt。

R4已确认在其中央stage批唯一注册`runtime/mod.rs`中的`admin_tax_projection`并真正调用core，纳原5个真实World测试进入实际unit构建/执行。当前该叶未注册，先前harness5绿不是生产mod交付；本轮正式ordinary11也不能代它。

## 保留的能力门

个人`BonusShares`仍typed不支持。R/R+1公司变动税日没有直接官方证据，下载challenge/550失败原件保留；本ordinary入口拒绝完整历史中任何公司或单位movement、旧式无最终确认绑定REG与未经核实限售资格，不能绕此门套ordinary。

本片若通过正式构建仍仅为候选来源边界，不是完整生产链完成。父任务负责后续R4接线和本地main合并，未经授权不push。
