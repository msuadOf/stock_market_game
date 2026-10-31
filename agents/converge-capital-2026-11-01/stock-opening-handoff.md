# 受限送转盘前接线交接

日期：2026-11-01。实施基线：`39dd842d0a5b826f1a651a1c808a70dbb8751729`。
用户授权继续修改初始六文件 WIP；未 reset、stash、操作其他 worktree／ref 或派遣。

## 本地切片

- 复用 `distributions_day_end` 的冻结分配、来源、Company 入账和 `mark_credited(E)`；
  新入口只处理 Registered 且 E 到期的送转，不把 E 的现金付款提前执行。
- `civil_cash_distributions` 将股份意图逐项绑定冻结来源和真实 Owner，准备 `ShareSplit`，
  检查原 operand 与完整最终 GroupAccount，保留现金、冻结、累计成本等其他字段。
- `civil_information` 同一 Admin 承载 Group 股份、Desk 参考价／镜像、Company／REG／终态账簿。
- REG `pending_opening` 保存原 frontier 与 movement；物理股份先入账，civil 不越过 D。
  E 收盘消费完全相同的 opening，单位相同的 NonTradingTransfer 也校验全部来源 lots。
  零分配 holder 保留零 movement 和 source_lots，不发零数量账户操作。
- Rust 严格可空必填字段与 Web schema 同步。完整恢复保留 Company 双向资本重放，
  只对精确绑定的 pending stock credit 放行次日事实，并验证唯一除权组、公式、账户及消费位置。

## 支持范围与拒绝

仅 native、单证券、Exempt、无个人税账、全流通、全部真实账户、无 Treasury／External、
无重叠公司行为。Drain／解冻后须无活动／待续行计划、未执行玩家委托、在途或冻结资源。
原 lots 漂移、错过日期、无登记、数量／价格溢出、重复 opening／除权显式拒绝。
单户单笔入股上限沿现有 Group undo 的 i32 数量边界，不修改 runtime 协议或造资金。
拆缩股、多证券、个人税、长期限售等仍未接通，不把本切片表述为全部资本行为完成。

## 已做与未做的验证

已做：Rust 改动文件 `rustfmt` 语法解析与格式化；三份 Web 修改文件 `node --check`；
`git diff --check`。这些静态检查成功，不代表 Rust 类型检查或业务测试通过。

遵照用户要求，**未运行任何 Cargo 编译／测试，也未运行 Node 业务测试**，由主任务统一执行。
先写测试再接实现，但本实施回合未实际运行 red／green，不声称已经获得运行结果。

主任务优先执行小单元／冒烟，普通命令和单 case 均 10 秒硬上限，编译独立安排并使用多核：

1. `registered_stock_distribution_can_precede_market_in_same_unit`
2. `same_unit_stock_opening_covers_zero_allocation_sources_and_requires_explicit_wire`
3. `native_stock_opening_real_registration_restore_first_sale_and_close`
4. `native_stock_opening_weekend_projection_abort_leaves_all_authorities_unchanged`
5. `native_stock_opening_real_close_projection_failure_seals_session_without_credit`
6. Web `save/schema/company/state.test.ts` 与 `repurchase-binding.test.ts`。

代表旅程只装配真实批准事实；R 真实撮合买入，随后生产登记／盘前到账，保存并严格恢复，
E 首拍由玩家卖出新可用股份，E 收盘消费 opening。另一条周末旅程在最终投影拒绝，核
Session／Group／Desk／Company／REG 均不泄漏本次 Admin，且可重试；真实收盘后失败另核 poison。

独立 Sol 完整 diff 复核由主任务单独安排，本回合未派遣、未取得复核意见，不宣称验收完成。

## 规则依据

实际重新读取上交所 2026 年修订交易规则发布页和 DOCX 正文，确认 2026-07-06 施行，
4.3.1 次一交易日除权，4.3.2 公式与行情前收参考价，4.3.3 涨跌幅基准。
正式文档链接及受限范围已同步 `docs/trading-rules.md` 末节。
深市与中国结算送转上市／碎股分配继续沿既有已登记证据，未宣称本轮重新取得指南正文。
