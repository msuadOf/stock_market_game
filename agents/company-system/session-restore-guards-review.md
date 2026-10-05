# Session restore guard 独立复核

日期：2026-10-06。复核范围：`session/persistence.rs`、`session/observation_clock.rs`、`session/company_corrections.rs` 与 `session/company_simple_session_tests.rs` 中本轮 restore guard、月报盘中游标及更正来源校验；结合 SaveSlot／`GameSession::restore`／`ProtocolSession::restore`、披露派发及公司模式契约阅读。未运行 Cargo。

## 结论

本次复核未发现可确认的 A 股时段、前视或跨模式 restore 缺陷。历史证据保留如下：`.tmp/checklist-wave4/host60-session.log` 为早期六绿二红；`.tmp/checklist-wave4/host62-session.log` 为缩量前九项通过、8.99 秒，接近普通测试十秒上限。缩量后 root fresh Engine 日志 `.tmp/checklist-wave4/host63-session.log` 显示九项全部通过、0.90 秒。复核者亲读该日志和当前源码，但未独立运行 Cargo；以下 fresh PASS 仅指该日志中的九个 session case，不代表完整 checklist 或其他 suite。

增量静态复核：`observation_clock.rs` 曾因误用 `rustfmt --emit stdout` 将文件路径标题写入源码首行，导致 Rust parser 错误；当前源码未见该标题，且本次精确修复仅删除标题及相邻空行，时钟公式保持不变。`restore_rejects_company_config_and_civil_date_drift` 通过 `config.environment.noise.monthly_bp += 1` 构造配置差异；该字段确为 `PeriodNoiseConfig` 的月度配置字段，继续验证 restore 拒绝 company config drift，断言未弱化。编译及原九项测试随后由 host62 实际通过，复核者没有自行运行 Cargo。

## 复核项

- A 股时段：观察时钟复用既有 09:15 开盘、连续交易时间压缩及 14:57–15:00 收盘竞价映射；日终月报保存游标保持 settled date 的 23:59:59，最后市场 tick 的特殊时刻为 15:00，二者不应混淆。
- 前视与时钟边界：月报盘中游标精确匹配已提交 tick 的 `observation_instant_at`；用例保留对同日未来秒 37800、54000、86399 的拒绝断言。`disclosure_checkpoint_handles_open_close_and_closed_day_without_relaxing_public_archive` 覆盖 2030-01-04（周五）两 tick 日的起始 tick 0、最后市场 tick 的 15:00 closing checkpoint、交易日日结至周六、休市日日结至周日，并分别验证季度与自定义月报下盘中低层 restore/hash 往返、公共 Protocol 拒绝，以及两个日终档案的公共 Protocol 接受。该覆盖与 ADR-0025 的仅日终公共存档边界一致；季度不触发月报盘中派发，周末自然日日结不伪造市场 tick。host63 日志记录这九项 suite 全绿。
- 模式串档：`validate_company_domain` 同时验证所选系统配置、发行人身份/股数映射、推进日期及报告 `SimpleGenerated` 来源；更正事实校验要求原报告来源、公司、standalone scope 与更正结果一致，符合当前 Simple only Session 契约。`simple_restore_rejects_foreign_model_publication_source` 继续断言初始化报告非空并篡改来源后 restore 拒绝；该项在 host63 九项全绿日志中通过。
- 作用范围：新增复用时钟公式可避免 restore 与运行时钟漂移，改动与披露 restore 守卫直接相关；本复核未审查工作区其他大量变更，也未将日志之外的测试结果视为已验证。

## 待补边界证据

新增用例已覆盖月报 `day_tick == 0` 开局、休市日跨日后 tick 0、月报日终公共归档、最后一个市场 tick 后尚未自然日日结的 15:00 checkpoint，以及季度日终公共归档。它从源代码层面覆盖了先前建议的关键组合；实际执行结果待 root fresh 验证，当前未运行 Cargo。建议后续在该用例中精确断言游标值：最后市场 tick 为 15:00，月报日终为 settled date 的 23:59:59，季度日终为 18:00。`closing` 仅在市场 tick 已跨入下一游戏日而 CivilClock 尚未结算上一自然日时成立（市场已完成会话数仍等于截至当前自然日的预期数）；日结后次日开市会增加预期会话数，休市则 phase 为 `ClosedDay`，两者都不会误命中 closing checkpoint。

为避免 8.99 秒场景受机器负载影响触及十秒上限，测试 fixture 改为零 NPC、十二个前史结算周期，第九项时钟专测局部采用零前史；其他模块的二十四周期 shared macro 未改变。独立复核确认来源篡改仍使用非空初始化报告，原游标负例、hash、Protocol 接受／拒绝断言均保留，没有放宽公共日终归档条件。root 随后以 host63 fresh Engine 执行缩量后的九项并全部通过，0.90 秒；复核者只依据该日志与当前源码审阅，没有独立重跑。

未修改实现代码、未运行 Cargo。增量 review 仅审计上述测试与 fixture；其他源码仍按前述范围复核，工作区其他变更不在本 review 范围内。
