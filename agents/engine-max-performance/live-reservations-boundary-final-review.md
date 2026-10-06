# LiveReservations 边界测试补充复核（2026-10-06）

## 结论

复核限定于 `live_reservations.rs` 指定的三项新增测试和 `account_book.rs` 的 `failed_resource_validation_is_not_cached_as_success`。四项单测均由已重编 binary 独立运行通过，每条使用 `timeout 9s` 与 `--test-threads=4`。生产实现未修改。

没有发现旧断言被删除或弱化、测试 fixture 违反明确的 A 股单位/整 tick 语义，或新增测试与需求无关的问题。三个 LiveReservations 测试分别验证唯一叶页 mutation 的 Weak 身份失效、failed projection 不安装 prepared rows、created row 经 rebase/reset 后派生预留正确，以及预热不改变 session hash/save bytes。AccountBook 新测试验证校验 closure 报错时连续调用仍重复校验，并且失败结果没有存入成功缓存。

## 对前审查四组补强建议的覆盖

1. **唯一 strong owner mutation、rebase/reset、hash/serde：大部分覆盖。** `warm_projection_rechecks_unique_mutation_and_does_not_install_failed_rows` 在旧 projection 仍存活时通过 `get_mut` 修改唯一 source leaf，检查新投影值、旧快照和页身份；`warm_reservations_follow_created_row_rebase_and_reset_without_affecting_hash_or_serde` 检查 created row、rebase、reset，旧快照值保留，rebase 后重新投影且新旧页值相等。session/save/hash 对应断言也由第三项测试覆盖。
2. **删除页、改变页、overflow 同时存在时的失败原子性：部分覆盖。** 测试先预热，然后修改已有行并插入造成现金溢出的行，确认失败后 cache 目录与页 Arc 未变，再修复输入并成功重投影。但该失败场景没有同时删除另一旧页；已有 `ledger_release_removes_page_without_ghosts_and_keeps_sealed_snapshots` 覆盖成功路径的末行删除，不等同于“布局删除与其他页失败同时发生”的原子性断言。
3. **Eq/serde/hash 与独立投影值相等：部分覆盖。** 新测试确认 ledger Eq、hash projection 序列化字节和 session hash/save bytes 不因预热改变，并确认 cache 字段不进入 ledger serde。没有显式从两个独立 source allocation 生成相同 `LiveReservations` 并比较，也没有比较相应 `DecisionResourceSnapshot` 的 Eq。现有检查可以证明所测预热路径不改变业务输出，但尚未完全落实前审查提出的独立 allocation 等价断言。
4. **AccountBook 失败不缓存及输入边界：部分覆盖。** 新测试确认相同失败重复返回且 cache 仍为 `None`。既有 `resource_validation_reuses_immutable_pages_and_invalidates_every_input` 保留成功页复用、价格/账户 mutation、现金与股份预留变化及 authority 隔离断言；每次构造相同空 fixture 也涉及同值、不同投影 allocation 的命中。新增测试没有显式覆盖“失败后换成成功并缓存”，也没有专门走完 `None → Some → None` 预留页切换。

上述是前次列出的非阻塞测试建议仍未完整落地之处；本次任务仅复核既有新增测试，不扩大修改范围。若将四组建议视作必须逐项实现，则不应声称它们已全部覆盖。

## 断言、fixture 与语义检查

- AccountBook 原测试只把 cash/sell 两张测试输入改为 `LiveReservations::fixture`；原有调用次数与说明断言仍保留：同页重复校验、账户页 mutation、价格变化使全部页重验、现金与股份预留只影响相应页，以及 shadow cache 不污染旧 authority。未见把强断言改成宽松断言。
- `Money::from_cents` 明确以分构造现金；股份参数及预留仍是整数股（`u32`）。新测试没有引入金额/股数混算或跨账户股份合并。
- 生命周期测试以 `EnvelopeLedger::insert_created`、`rebase_private_for_tick_commit`、`reset_tick_state` 覆盖同一 tick 的 created-row 重置；不把 terminal evidence 当作下一批 live 预留。`Envelope::created_at_validation` 是内部测试 fixture 构造器，测试没有据此主张真实委托校验或清算规则。
- AccountBook 的失败测试使用显式 `StepFatal::InvariantViolation` closure 来隔离缓存安装行为；它不是交易规则错误 fixture，也未伪装真实资源超限场景。

## 测试记录

以下四项均通过，单项耗时均低于 1 秒：

- `warm_projection_rechecks_unique_mutation_and_does_not_install_failed_rows`
- `warm_reservations_follow_created_row_rebase_and_reset_without_affecting_hash_or_serde`
- `prewarming_nonempty_reservations_keeps_session_hashes_and_save_bytes_identical`
- `failed_resource_validation_is_not_cached_as_success`

未运行 Cargo、benchmark、GDB、browser 或额外测试。
