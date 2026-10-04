# G35 / G41 经营闭环实施记录

## 范围与现行语义

- 工作目录：`.worktree/implementation-audit-final`；不操作 Git index，不提交，不修改总账核销状态。
- 原证据：`agents/implementation-audit/implementation-audit-2026-10-02.md` 的 G35/G41/Q23，`reaudit-engine.md` 的 G35/G36，`exhaustive-review/luna03.md`、`luna54.md`。
- 已读根 AGENTS、工程原则、ADR-0023/0024/0025、公司会计登记、公司经营计划 K2/K3、Task 8/14 及现有经营调用链。未引入现实行情、股东分配、投资者补钱或资金循环。
- 会计依据沿用 `docs/company-accounting.md` 的既有官方核验/受阻登记日期。未重新联网核验或解除 CAS 18、现行税法等阻塞；新增日结收付节奏明确登记为游戏简化，不宣称真实法定预缴及汇算清缴期限。

## 已实施

1. G35：真实 `CompanyOperations::advance_civil_day` 在月末经营之后折旧，12 月 31 日折旧后执行一次年度所得税计提并尝试支付应交余额；未付税款后续自然日继续重试。合法零额折旧推进寿命但不生成零金额凭证、虚构事件 ID。
2. G35：工商/地产真实商业借款以持久 `DEBT` 到期队列付息还本；地产保存原有借款入口的 `maturity_date`。银行新存款及显式配置的期初已有存款统一进入 `DEP` 到期支付。先计息再到期收付；不足时保留本金/利息，后续自然日重试，不续贷或补钱。
3. G41：真实付款失败按发生日进入 `CompanyOperations.payment_failures` 权威历史，参与 serde 与 Session checkpoint；18:00 披露派发读取当日历史，在不可变 PublicLibrary 中形成带请求金额/事项的 `PaymentFailure` 结构公告。
4. G41：恢复校验拒绝未来失败、空列表、未知公司、空事项、非正金额、日历范围外日期和把付款失败伪装为 active 经济冲击。公告创建和恢复同样验证事实窗口/正金额。
5. 协商的 G36 接缝：四行业 `books_mut` 结账委托；生产采样、显式冲击注入及公告过滤行业不适用事件。银行不接商品需求/成本，银行/保险不接生产中断，固定资产减值仅工商；保险/地产信用恶化仍保留已登记的只记录风险简化。
6. `session.rs`、`persistence.rs` 由 `integrate_session_contracts` 独占接线；Web strict parser/type bridge 由 `integrate_web_save_contracts` 同步新增字段与公告形状。集团披露 helper 由 `implement_company_assembly` 提供，本 worker 合并其 `DayEndDisclosureCtx.groups` 与 scheduled consumer。

## 事务与未改约

- Q23 公共 `IndustrialBooks::accrue_income_tax` 的重复直接调用语义不在本批全面改约。生产日期必须逐日，全年计提只在 12 月 31 日执行一次。
- 直接 `CompanyOperations::advance_civil_day` 原有 Err 保留局部写入契约未变，调用方不得在不恢复 checkpoint 的情况下重试；真实 Session 日结外层强事务覆盖新增历史、税费及折旧。
- 真实 Session 测试在年末经营结束后以 CivilClock due 序号耗尽制造镜像同步失败，验证整个 save 字节不变，再恢复合法 clock 并重试，只产生一次当日所得税计提及一次月折旧。
- 付款失败是已确认的未完成请求，不等同于现实法定重大违约、停牌、退市或破产。`PaymentFailure` 事件窗口不表示次日债务解除。未新增强制个人获知或自动估值更新，Q11 不在本批冒充核销。

## 定向验证

- 构建：独立 `.tmp/company-ops-target`，`cargo test -p engine --test company_operations --test industrial_accounting --no-run -j 16`。仅构建与短测，没有完整回归或长期市场验收。
- 红证据：零额折旧原实现因零金额分录被拒；独立复核补充的期初已有 DEP 测试在修复前因到期事件缺失真实失败。其余初始新增测试先于实现写入，并在缺少期末 hook 时编译失败；后续补充的恢复/公开/Session 边界测试不冒称逐项重新跑过红阶段。
- 当前短测：`maturities::` 5、`period_end::` 3、`payment_risks::` 5、`industry_guards::` 2、`session_boundary::` 3、`seam::` 1，以及 `industrial_accounting assets_gold::` 4，共 23 case。每命令外部 10 秒 deadline，`--test-threads=8`，不同 filter 独立子进程并行。真实 Session 三 case 合计约 1.73 秒。
- 初次税期测试夹具使用已过到期日的 opening debt，构造失败；已改为无债工商且显式合成盈利参数，不修改生产代码以迎合坏夹具。
- 新局工厂为生成实际会计前史执行其既定初始化过程；这里只验证代表性 Session 边界，不进行多年月序历史回归或市场模拟。
- 独立 reviewer 曾误运行整个旧 company_operations binary，37/37、8.24 秒，包含 history_gold。此结果仅如实记录为 reviewer 操作，不作为本 worker 的长历史验收或完整回归证据。

## 独立复核

未参与实施的 reviewer 审查完整自有 diff，指出期初已有 DEP 缺少排队及付款失败历史缺少日历下界；两项均修复并复验。最新新增 DEP + Session 四 case 独立验证 4/4、1.74 秒，通过，记录见 `company-operations-review.md`。G35/G41 自有经营闭环及协商 G36 接缝门禁通过；整批最终状态仍由 root 核对共享接线与并行任务，不把本记录当作所有剩余项验收。

## G36 联合复核补充

assembly reviewer 指出原 `industry_guards` 的 sampling `all()` 断言可能在银行没有
active shock 时为空真，不能单独代替真实 Session 公告证据。随后只增加
`industry_sessions.rs` 两条测试和复用 Session fixture helper，没有再修改产品逻辑：
银行选择能够实际激活 CreditDeterioration 的显式 seed；保险使用 1 group、2 日保障的
代表性短 fixture 与确定性高候选概率。两条测试都要求当日适用 active 事件非空，并由
真实日结 Event 的 PublicationId 查询 PublicLibrary，要求适用经济公告非空、没有
ProductionInterruption/AssetImpairmentSignal，银行只公布适用信用事件。

实施者短测 2/2、1.23 秒；`review_company_assembly` 独立复验 2/2、1.26 秒，并纳入
`company-assembly-review.md` 的 G36 联合 PASS。加上前述 23 case，当前定向短证据共
25 case；新增两条测试不是完整历史/全宿主验收。日志位于
`.tmp/company-operations-logs/industry_sessions.txt`。
