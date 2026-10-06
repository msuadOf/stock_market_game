# 股价 seed 初始化独立核查

日期：2026-10-06。只读核查已提交实现；未运行 Cargo、未改源码或 Git index。工作区存在其他未提交改动，本核查不据此评估。

## 结论

实现已接入 Web 的新局 seed 预览，不只是文档勾选。`company-initial-preset.ts` 以 `initial_price`（分）乘 `total_shares` 得到虚拟市值，并由 `seed + stock code` 确定虚拟 PE（10–30）和 PB（1.00–3.00）。

收入按所选结算期间长度折算。虚拟假设为固定开支占收入 20%、变动开支占收入 60%、所得税率 25%；对应税前利润率 20%、税后利润率 15%。收入公式年化后满足 `期间净利润 × 12 / periodMonths = (股价 × 总股本) / virtualPE`，精确金额采用分和 half-even 舍入。权益满足 `期初权益 = (股价 × 总股本) / virtualPB`。预设不直接写入目标净利润，而由收入、开支和税务流程形成利润。

`createSeedDraft` 生成预览配置；新局命令使用用户当前草稿，测试断言安装的 `company_system` 与预览完全相同。修改 seed 会重生成 preset；`custom` 草稿不被 seed 重抽样覆盖；更改周期按新周期重生成同 seed 的基准，custom 周期修改显式拒绝。Engine 的 `CompanySystem::create` 只消费配置和 seed，不读取股价；`GameSession::restore` 以存档 setup/seed 重建，并校验配置与财务状态，不从当前或成交股价重新校准。因此初始化外没有股价重锚路径。

这属于虚拟开局的初始匹配假设，不是现实定价、现实 PE/PB 规律或现实开盘表现保证。市场撮合仍可令开盘价格上涨或下跌；实现也没有承诺开盘无涨跌。

## 证据与边界

- 设计规则：`agents/remaining-questions-and-features/q14-financial-model-design.md` 第二章 §1–§5；实施状态及边界：`agents/company-system/implementation-checklist.md` 全文第 6 节及验收声明；原则与方案：`AGENTS.md`、`docs/principles.md`、ADR-0035、ADR-0036。
- 核心实现：`apps/web/src/config/company-initial-preset.ts`、`apps/web/src/config/seed-draft.ts`、`apps/web/src/config/defaults.ts`、`packages/engine/src/company/simple/state.rs`、`packages/engine/src/company/simple/finance_posting.rs`、`packages/engine/src/session.rs` restore 路径。
- 初始化数值、周期、seed 预览和编辑边界测试：`apps/web/src/config/company-initial-preset.test.ts`、`apps/web/src/config/seed-draft.test.ts`、`apps/web/src/app/company-config-commands.test.ts`、`apps/web/src/components/company/company-system-config.test.ts`。本次只读审阅测试代码，未执行测试。
- **原测试证据缺口已关闭：** 新增 `company-initial-preset.test.ts` 恒等式用例覆盖 seed `0`、`42`、`u64::MAX` 与 1／3／6／12 月期间；按 half-even 舍入重算费用和 25% 所得税，断言年化税后净利与 `市值 / virtualPE` 的交叉乘积误差在显式舍入上界内，并断言 `virtualPB × 期初权益` 与市值在半分舍入界内。复读完整 diff 后未发现口径或边界错误。`.tmp/company-system/session-actions/seed-price-confirmation.log` 记录 preset、seed、command 接线及恒等式用例共 14/14 通过；本复核读取日志，未自行运行测试。`git diff --check` 无输出。
