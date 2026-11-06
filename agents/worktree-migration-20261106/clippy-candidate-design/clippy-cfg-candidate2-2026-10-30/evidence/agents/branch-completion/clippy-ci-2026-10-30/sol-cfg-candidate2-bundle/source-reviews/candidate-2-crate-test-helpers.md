# 仅 crate 内消费者的八个测试 helper

此片从固定50d49370基线独立形成，位于callsite-fix树，与candidate1源码冻结分开。源码仅7文件各函数加cfg(test)，没有改实现体/断言；完整check仍待统一窗口。

已逐packages/engine/src、engine/tests、engine/examples与apps Rust引用核对：

- civil_information::with_prepared_runtime_civil_day：只有ordinary_tax_day_end/payment_runtime_tests，生产仍保classified变体。
- administrative_mirror::apply_administrative_operations：只有administrative_mirror_tests；生产civil/repurchase各正式入口保留。
- SessionCompanies::legacy_mut：所有调用均为独立测试文件，另causal.rs中的consumer位于cfg(test) mod tests（line839）。
- runtime_day_end_mirror_rollback：只有failure_tests。
- technical_observations_read：只有runtime/drive_tests，生产纯构建器保留。
- working_orders_for_accounts：只有本文件cfg(test) working_orders_by_account的调用，生产working_orders_for_accounts_on保留。
- PlanBook::end_trading_day与apply_active_events_atomically：仅origin_tests、continuation_tests与本文件cfg(test)模块；生产按股日界与现行反馈应用保留。

这些符号全部pub(crate)/pub(super)/pub(in crate::session)，不可能由engine integration tests或其他crate外部消费；实际feature消费者也按其外围test cfg核过，不单靠文件名断定。A股T+1/计划归属/公司执行未接通门禁仍由原production核心守卫，函数体没有更动。

本片由主cfg实施员实施，待上级未实施者完整diff独审。指定rustfmt与gitdiffcheck通过，未Cargo。patchsha256=`53ff318c9293fa5963bf4883b891c349621249eec38e83d14e954421a8095f52`。
