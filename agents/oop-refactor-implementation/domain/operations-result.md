# operations 实施记录

- 基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`。
- 范围：`domain-R2-N19`、`domain-R2-N20`；产品改动仅位于 `packages/engine/src/company/operations/`。
- 当前状态：生产迁移、保护测试与独立静态复核已完成，见 [operations-review.md](operations-review.md)；root 最终编译与 7 个指定短 case 已通过，证据见 [final-summary.md](final-summary.md)。
- 开工读取：AGENTS、principles、testing、architecture、open-questions、ADR-0016/0023、两个 action 的完整正文、operations 全部 12 个源码文件；补读 spec、scheduler、events、相关行业配置和工商费用路径。
- TDD 如实记录：先写入 6 个行为保护测试，再迁移生产代码，随后补第 7 个采样顺序测试。重构保护测试预期基线通过；按父任务约束没有运行 Cargo，未观察基线结果，也没有声称 red/green 已通过。
- 未运行 Cargo、产品测试、Git 写命令或全仓 fmt。仅对本人改动的 config/core/day 三文件运行 `rustfmt --edition 2021`；静态 diff 已逐段对照基线。

## domain-R2-N19

- 文件：`operations/config.rs`、`operations/core.rs`、`operations/day.rs`。
- owner：私有 `IndustryPairView<'a>` 仅持有四行业各自的 `&mut Books`/`&FlowParams` 配对借用；长期 books/params 继续由 `OperatingCompany` 分立持有并序列化。
- 方法：`at_build_guard` 保留 build 独有的 spec.kind 检查；`at_existing_guard` 保留 day 原 books/params 检查；`advance_day` 消费 view 与 `FlowDayContext`，静态分派四行业既有 `advance_day`。
- 真实 caller：`CompanyOperations::build` 在 spec.validate、duration 验证后建立 view；`OperatingDayRun::advance_company_flows` 经 `advance_flow_day` 在原 day guard 处建立 view，再直接调用各行业流。
- 行为保护：duration 首错、错配公开 DTO 和存档仍可 Deserialize、day 错配在 expiry/sample/due 后失败、spec.kind 的错配不被加入 day 新 guard。公共字段、derive、JSON 形状、金额单位、经营 units 与证券 shares 边界保持。
- 验证状态：root 指定短测通过，独立静态复核已完成。没有新增 trait、默认行业或永久受检存档 owner。

## domain-R2-N20

- 文件：`operations/day.rs`；`dispatch.rs`、`injections.rs` 保留现有到期与利息 owner 方法，未做无必要的额外迁移。
- owner：私有 `OperatingDayRun<'a>` 借用原 `&mut CompanyOperations`，拥有 date、entries_before、activated、expired、payment_failures、dispatched_due 的本日生命周期；没有复制或新增可序列化 RNG/账簿/日期状态。
- 方法：`begin`、`validate_date`、`advance`、`expire_shocks`、`sample_and_activate_shocks`、`dispatch_due_actions`、`advance_company_flows`、`schedule_next_interest`、`build_day_report`。
- 真实 caller：`CompanyOperations::advance_civil_day` 创建并运行本日 owner；owner 调用原 `CompanyOperations::dispatch_due_on`（按 due_date/id 依序派发 InterestAccrual/ContractMaturity）和 `submit_rolling_interest`；前史/live 仍共用同一公共日推进入口。
- 顺序：cache 失效 → 日期守卫 → entries_before → expire → 市场/行业/公司 sample 与 activate → dispatch_due_on → 公司 id 序行业 flow → date.next → 次日 interest 排队 → next_expected → 分录差额报告。
- 行为保护：错误日期仍先失效 hash cache；当日 AR 回款可支付之后的经营费用；次日利息仅在 flow 成功后排队；flow 错误仍保留回款/队列 settled floor，日期不前进；市场/行业/公司输出顺序与每路 RNG 状态保持。
- 领域语义：推进的是自然日，含周末；公司经营资金仍与投资者账户隔离；不引入分红、融资、回购、补钱、证券 T+1 或板块规则。
- 验证状态：root 指定短测通过，独立静态复核已完成。没有把 Err 后的部分写入改为事务回滚。

## 统一验证交接

- features：无新增要求；新测试可使用 default features，root 的统一 feature 集也可执行。
- 新 lib filter：`company::operations::day::tests`，共 7 个短测试：
  1. `duration_error_precedes_pair_guard_and_mismatched_dto_deserializes`
  2. `restored_pair_mismatch_keeps_prior_expiry_sampling_and_due_consumption`
  3. `restored_spec_kind_is_not_an_extra_daily_pair_guard`
  4. `shock_records_follow_market_then_sorted_industries_then_sorted_companies`
  5. `wrong_date_invalidates_projection_before_rejecting_without_business_changes`
  6. `same_day_maturity_funds_flow_then_only_next_day_interest_is_queued`
  7. `flow_error_retains_prior_maturity_and_does_not_queue_next_interest`
- 既有集成入口：`packages/engine/tests/company_operations/main.rs`（Cargo test target `company_operations`）；建议 filters：`determinism`、`failures::flows`、`shock_gold`、`risk_gold`，四行业装配与分派依赖既有 fixture 的真实经营行为保护。
- 各 case 仅一至两个公司、一次日推进，没有前史长跑。普通测试整命令/单 case 仍须遵守 10000ms 硬上限，线程预算与构建阶段由 root 统一记录。
- 独立静态复核已完成；root 运行反馈已取得，指定短测通过；完整回归未运行。
