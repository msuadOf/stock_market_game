# CompanyOperations 独立复核

日期：2026-10-04。复核者未参与产品实现；本记录不修改产品代码。

## 范围与方法

审查 G35/G41 及协商的 G36 接缝：`company/operations` 全部本批 diff、新增 `settlements.rs` / `maturity_payments.rs`、`company/events.rs`、工商折旧、行业账套 `books_mut`、地产借款到期字段、公开公告、日终披露、新增经营测试和本批文档段落。共享 `session.rs` / `session/persistence.rs` / Web strict schema 只核对真实 consumer，不将其他任务修改归入本批。

已阅读工程原则、开放问题与 ADR-0016。没有新增或修改证券撮合、申报单位、T+1、涨跌幅制度。

## 初审发现

1. **P2：初始化银行账套的已有定期存款漏排到期（已修复、复核通过）。** 初审时 `CompanyOperations::new` 和 `generate_history` 仅扫描工商/地产 DEBT；DEP 只在 `bank::advance_day` 新吸收时排队。修复后 `schedule_debt_maturities` 同时扫描已有未清偿 DEP，跳过已结清和 pending，按 `maturity.max(first_due)` 排队；已有存款首日到期回归通过。实施者报告修前该回归因缺失 due 断言真实失败；复核者独立确认修后通过。
2. **新增付款史缺少日历下界校验（已修复、复核通过）。** 修复后 SaveSlot consumer 显式检查 `init_only_min_start` 至 `runtime_max_end`，保留合法前史，不误用 runtime 起点。真实 Session 的 policy 外日期拒载回归通过。

## 三项门禁结论

- **大 A 语义与依据：** 新增月末折旧、年末所得税、到期收付、次日重试和 18:00 付款风险公告明确登记为游戏简化；公告不是现实重大违约、罚息、破产、停牌或股东清算。所得税 CAS 18、固定资产 CAS 4、现行税法等原有取证阻塞未解除，不把此次调用接线视为现实法源核验完成。投资者资金与经营账套未混流。
- **必要性与范围：** 分拆 settlements / maturity_payments、地产 maturity_date 权威字段、PaymentFailure 日期键历史及公开结构均直接支持缺口，不引入依赖。其他行业 `books_mut` 是原有统一 Closing consumer 必需接缝。没有发现本批无关产品扩张；Q23 公共税 API 幂等问题仍未改约。
- **边界与跨层：** 日结先计息后付息还本、付息失败不继续还本、保持未偿余额并重试；合法零折旧推进寿命而不造零凭证。`next_expected` 守卫与 SaveSlot 时钟一致性禁止已成功生产日期重复计税。Session `end_civil_day` 使用 checkpoint 恢复整个 Arc CompanyOperations 与披露状态，故后续失败不会留下已计税/已付款史半写入。`validate_company_domain` 确实调用付款史 validator；公告创建/恢复均校验金额、窗口、振幅；Web strict schema 已新增字段而非静默默认。初审两项发现均修复并独立复核，本批 G35/G41 与协商 G36 接缝门禁通过；该结论不是全部历史实现、法源阻塞或全宿主验收完成声明。

## 验证记录

复核使用已编译的 `.tmp/company-ops-target/debug/deps/company_operations-4a14711434dbcaa4`：外部 `timeout --signal=TERM --kill-after=1s 9s`，`--test-threads=8`，37/37 通过，binary 报告 8.24 秒。未自行启动编译或全量回归。复核误用了整个短 binary 而非仅新增测试过滤，包含两个 `history_gold` case；该事实已报告实施者，不能将其称为历史全量验收。

新增现有测试覆盖月末与零折旧、年末一次计提支付、跨年欠税不重计、工商欠本重试、地产到期 caller、DEP 本息与欠本重试、付款史 serde、日终公告一次发布以及异常历史/公告拒绝。

修复后独立复验：并行启动两个过滤命令，分别 `session_boundary::` 和 `preexisting_bank_deposit`，均使用外部 `timeout --signal=TERM --kill-after=1s 9s` 及 `--test-threads=8`。结果为 3/3 与 1/1 通过，共享实际 wall-clock 1.74 秒；不包含历史长回归。真实 Session 新增用例验证日终付款史保存、恢复与公开查询，policy 外日期拒载，以及税和折旧已执行后 Clock sync 失败的整局字节原子回滚；恢复 Clock 重试后当日仅一次 TaxAccrual，资产剩余寿命恰为 84 月。

## 批准后文档增量复核

仅复核注释与记录增量，没有重新运行测试。`config.rs` 对四行业权威 Books 委托的描述、`dispatch.rs` 对先计息后到期两阶段及 DEBT/DEP 引用的描述与已批准实现一致。`docs/company-accounting.md` 新增 PaymentFailure 当日窗口不表示次日债务解除，正确区分不可变事件发生日与持续欠款状态。

`company-operations.md` 准确保留 Q23 公共 API 未改约、直接 Operations 非事务与 Session 外层事务差异、法源阻塞未解除和 reviewer 误跑历史 case 限制。查阅 `.tmp/company-operations-logs`：最新构建成功；maturities 5、period_end 3、payment_risks 5、industry_guards 2、session_boundary 3、seam 1、assets_gold 4，实际共 23/23 通过。此为实施者最新日志核对，区别于复核者此前独立执行的 4/4，不冒称全量历史或长期验收。文档增量复核通过，原批准结论保持。

## 第二位非作者交叉复核（待最终验证）

复核者 `/root/review_company_operations` 独立追踪 G35/G41 原始总账、`reaudit-engine.md`、`luna03.md` / `luna54.md` 与会计已决边界，以及全部经营、公开公告和共享 Rust / Web consumer diff。同样确认初始配置已有 DEP 漏排队；实施者正在补修，尚未将修后结果判为通过。

共享 Rust restore 已加入付款史日期 `init_only_min_start..runtime_max_end` 校验，保留合法初始化前史及逾期合同；这不是证明编辑存档付款来源的历史真实性。付款史不成为 active economic shock，不引入 RNG 消费；公开金额仅为失败请求金额，不包含现金余额、未公开账簿或完整公司账套。分批本息支付可能先付息成功、还本失败，属于显式支付顺序，不承诺整笔本息事务。

动态证据：2026-10-04 在目标 worktree 运行 `timeout 10s node --test --test-timeout=10000 --test-isolation=none --test-concurrency=8 apps/web/src/save/company-contracts.test.ts apps/web/src/save/company-books-schema.test.ts`，11/11 case 通过，Node 报告 128.17ms。先前同文件隔离模式命令显示 2 个文件通过，不能将文件数冒充 case 数。未跑完整回归、浏览器或三宿主验收。

仍待作者提供真实 Session 日结→公开查询→SaveSlot 恢复的代表性短测，以及期末日结 late Err 后 checkpoint 回滚/重试证据。直接 `CompanyOperations::advance_civil_day` 明确是非事务 API，Err 保留此前阶段写入；不能由成功日期重入拒绝或 serde 往返测试推导任意错误后可直接重试。生产 Session 的 checkpoint 静态接线已确认，但尚不以读码代替上述新增业务的动态覆盖。

## 第二位非作者修后最终复核

上述待办在本节核验：已有 DEP 已加入统一初始化/每日未清偿扫描，按合同到期日或首个可运行日排队，避免重复 pending；原有失败重试保留。付款史 policy 边界已由真实 `GameSession::restore` 短测覆盖。新增 `session_boundary` 用生产 Session 日结生成失败事实、18:00 公告事件、公开 Library 查询并恢复 SaveSlot；年末注入 CivilClock 到期序号耗尽，在经营已完成后的同步阶段失败，整个 save 字节回滚，修复时钟后重试只生成一次当日税分录、固定资产寿命不重复扣减。两个有效发现及真实入口覆盖要求均已解除。

独立动态复核使用作者通知 ready 的最新 `company_operations-4a14711434dbcaa4`，六个互不冲突过滤进程并行，每个 `--test-threads=8`，外部 `timeout --signal=TERM --kill-after=1s 9s`（为 kill 留 1 秒，总 deadline 10 秒）：`maturities::` 5/5、`period_end::` 3/3、`payment_risks::` 5/5、`industry_guards::` 2/2、`seam::` 1/1、`session_boundary::` 3/3，合计 19/19，最慢 1.70 秒。日志为 `.tmp/review-company-<filter>.log`。另对 `industrial_accounting-5cd42f3653d2918d assets_gold::` 同参数运行 4/4，通过且小于 0.01 秒。此轮没有执行 `history_gold` 或完整回归。

**最终裁定：G35/G41 本批独立门禁通过，无剩余阻断发现。** 大 A 语义依据沿用已登记官方资料与显式游戏假设，不把 CAS 18 / 税法取证受阻解除，不扩大为真实税务制度合规；必要性与最小范围通过；边界、跨层与非事务/Session 回滚承诺保持诚实。Q23 直接税务 API 的幂等性仍是原有独立边界，未以本次生产 once-per-successful-day 接线冒充修复。长期运行、完整回归、真实浏览器与三宿主验收均未执行，本结论不覆盖这些验收。
