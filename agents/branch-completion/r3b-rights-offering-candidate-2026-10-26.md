# R3b 配股／增发日终纯候选

基线 main `b172c1ab`；独占树 `.worktree/rights-offering-r3b`；分支 `feat/b4c-rights-offering-r3b`。
本记录不是生产接线验收；本片只含 child module、测试、最小 mod 声明与能力登记。

## 冻结 API

`rights_offering_day_end_candidate(&SessionCorporateActions, CivilDate, &TradingCalendar,
&BTreeMap<AccountId, RightsFinalResources>, &BTreeSet<AccountId>)`
返回 `Result<RightsOfferingDayEndCandidate, RightsOfferingCandidateError>`。

- `RightsFinalResources { available_cash: Money, kind: AccountKind }` 来自已安装的最终组端资源，
  不是总资产或未释放 held 资源。个人税账资格集仅包含当前模式确实允许的账户。
- 输入只读，返回本簇窄字段：registries、dividend_tax_books、rights_offerings、queue、rejections；
  R4 按顺序衔接 R1/R2/R3 候选，不能用旧快照覆盖其他片的变更。
- `RightsAccountResult` 以 event/account/submitted_on 关联 requested/paid/waived/refunded，
  `PreparedSubscription`／`PreparedSettlement` 均为待行政事务确认的候选，不是实际成功回执。
- 三类意图 `PaymentDebit`／`FailureRefund`／`NewShares` 不借用股利或税款操作。
  新股意图逐账户保留原 subscription.paid_amount：后续成本操作一次加入 invested，不重复扣现金或佣金。
- `RightsFinanceSettlement` 只输出 issuer/event/settled_on/issued_shares/proceeds，
  不直接调用 CompanySystem。外部 holder 的分腿保留在领域 settlement.holders 与名册中。

## 不变式与集成前置

1. 复用领域 plan/calendar/entitle/record_subscription/70% failure/settle 算法；不恢复旧整包。
2. 同日已受理显式队列先处理；任何显式意图优先于 NPC 默认策略；缴款首日才运行默认策略。
3. 同一账户跨事件按冻结 book 顺序共享局部余额，失败退款也作为本日资源意图递延后续事件。
4. 截止日处理完当日回执再关窗；同日重放不生成重复现金或新股意图。
5. 缺账户、负资源、重复申请身份、错过关键日期及溢出显式报错；失败不改变输入对象。
6. 首次取得定向新股的个人可从前一自然日空基线开税账；已持股而缺个人税账必须报错，不能补空基线。
7. `validate_rights_offering_capability` 是 Session 层长期限售门禁。域模型保留表达与纯算法支持；
   新候选拒非空 lock_until，后续生产命令和恢复入口必须接同一门禁，未接线不能宣称闭环。
8. runtime 的付款、退款及 SharesWithCost 仍需另批扩展既有 AdminTransaction；
   不能借本候选引入第二个提交／回滚框架。

## 官方材料与适用边界

2026-10-26 实际取得官方 PDF 并逐节读取；未凭 HTTP200 推断正文：

- 中国结算深业〔2025〕68号，118页，第2.4.3（二）与（五）、（六），第2.4.4（一），
  本机 `.tmp/rights-offering-r3b/sz-issuer.pdf`；SHA256
  `e8db1f9761b083542d72568a25dcbab02b5d0e86d41309ca108eb362c822e902`。
- CSRC第206号令，2023-02-17施行，30页，第53与59条；正文页附件相对URL需含双层
  `c7121921/7121921/files/`，两次错误URL收到HTML，已保留失败产物，不把其当PDF。
  成功产物 `.tmp/rights-offering-r3b/csrc206-correct.pdf`，SHA256
  `5871e185c144a7b8c3fc10148e2a19e277b7f4dcedfa0651f4c57e63610b010d`。
- 官方链接、沪市证据缺口、利息等简化已登记 `docs/trading-rules.md`。
  既有研究还记录2026年第6号适用意见修订；本片不改变第53/59条条件，也不宣称穷尽现行发行监管。

## 测试与资源证据

所有产物留在仓库根 `.tmp/rights-offering-r3b/`，独占 target；未运行全回归。
构建均 `-j16`、外层 `run-long-validation.mjs 300000`；普通测试二进制外层
`run-with-deadline.mjs 10000`、`--test-threads=8`，整套短 fixture deadline 同时约束每 case。

- `red-build.log`：147秒，建立可编译 stub。`red-tests.log`：3红，根因是候选没有缴款/关窗行为；
  红灯提交 `e353c640`。其中截止日 case 在缺结果时触发空索引，其余断言明确失败，未当编译错误红灯。
- `green-build.log`：第一次实现编译失败（u64股份与现 Money::mul_shares(u32)不合、错误日期API），
  `green-build-2.log` 修正后编译通过（62秒）。`green-tests-1.log`：11通过，0.03秒。
- `extended-red-build.log`：43秒。`extended-red-duplicate.log` 与 `extended-red-tax.log`：
  分别捕获重复 request key 与已有持股税账缺失两个真实红灯，随后修复；完整绿灯待下一 warm 构建窗口。
- root 2026-10-26 通知 I/O PSI 与磁盘压力后，没有新开冷 target；等待其他 slice 现有 warm 编译结束再验证。

独立完整 diff 复核尚未完成，修复后由 root 派未实施 Sol；本片不宣称完成。

### 最终冻结验证

- `extended-build-2.log`：46.47秒，`extended-tests-2.log`：18绿1红；新增红灯精确证明
  「显式 NPC 请求被拒后默认策略再次买入」，已通过保留显式账户集合抑制自动回退修复。
- `final-build.log`：39.99秒，同一 warm target，`-j16`、外共享300000ms deadline。
- `final-tests.log`：19个候选 case 全绿，0.03秒；`domain-tests.log`：既有配股领域11个 case 全绿，0.01秒。
  两条普通命令各有10000ms进程树 deadline、8个测试线程。未运行全回归、生产接线或混合日历验收。
- `rustfmt --edition 2021 --check` 两个新增 Rust 文件与 `git diff --check` 均通过。
- 最终业务源 SHA256 `cb3f366f8b69cd3a77cb8821e5e9db4125005c96833c9caf83fbaf9e5c8632c4`；
  测试源 SHA256 `819a8919e012f53bcdf8ac030d73e5ed042bb4c2e33ac3b4b2ee4aff8ac762e3`。
- 测试二进制由上述源码在独占 warm target 构建，未复制归档大 binary。现有工程警告保留在构建日志，未宣称无 warning。

完整 diff 已冻结交 root 安排未实施 Sol 独立审查；review 前不宣称批次完成。

## 独立复核 R1 修复：已安装拒绝事实的同日恢复重试

未实施 reviewer `sol61_review_plan_core` 给出真实组合反例：缴款首日 Retail 可用8800分、
权益10股但显式申请11股，首次被拒且无意图；安装候选清空队列后，同日重试会被默认策略
重新认购10股。原19例只分别覆盖首次拒绝与截止日重试，没有覆盖该组合。

先保存纯测试提交 `01df320610df34b27ba36140dbe7fa6b7fe82bac`，fixture 加入
first→install→Serde roundtrip→两次 retry。`review-r1-red-build.log` warm 编译42.22秒；
`review-r1-red-test.log` 真实断言 `retried.results.is_empty()` 失败（0.01秒、exit101），
未回填红灯。`review-r1-red-binding.json` 绑定当时3个源文件hash、命令、构建exit与binary hash。

实现只增加6行：默认策略同时检查持久化的同 event/account 且 `rejected_on == day`
拒绝事实；保留本次 pending 显式账户抑制，不以全历史拒绝永久封禁，不增加schema。

修复后 `review-r1-green-build.log` 同一 warm target 编译35.98秒（外300000ms、`-j16`）；
`review-r1-green-tests.log` 20个候选case全绿0.04秒；`review-r1-domain-tests.log`
既有领域11个case全绿0.01秒，两条普通命令各外10000ms、8测试线程。格式与diff空白检查通过。
机器可核绑定见 `.tmp/rights-offering-r3b/review-r1-green-binding.json`：

- 业务源SHA256 `74045e6839ecae2b3c20d41cbf3b2c0e74afe8d93d554b708133b19b8f0c54ff`；
- 测试源SHA256 `1f0fde28508a410c84f40576be6735d74800ad6815de8a51b69268325a7cac8f`；
- binary SHA256 `45ba3c7cf892ea8f13eed5cbfb001a9e1b00e1afe2342358204d017b8ab4facf`。

上述构建窗口由 root 在身份runtime编译结束后放行；没有新增冷target或复制大产物。
已交回同一未实施 reviewer 复核，仍不把修复完成等同于独立门禁通过。
