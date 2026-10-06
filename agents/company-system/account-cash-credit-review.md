# Account 现金贷记独立复核

## 范围与依据

复核人未实施 `packages/engine/src/account.rs` 中 `Account::credit_cash` 与新增的两个 `shadow_tests`。审查对象是该文件完整新增差异，动态证据为 `.tmp/company-system/session-actions/fresh-0.log`。

## 结论

静态复核通过。`fresh-0.log` 中账户 shadow 定向测试 `5 passed; 0 failed`，包含本次新增的现金贷记两项测试。本记录仅签核账户现金贷记及候选状态隔离，不扩展为公司行动或分红完整资金链的验收结论。

## 复核要点

- `credit_cash` 拒绝零及负金额，并以 `Money::add` 进行 checked 加法；溢出以 `AccountError::MoneyErr` 显式返回。
- 写入共享状态前先计算加法结果。非正金额或溢出时不会调用 `Arc::make_mut`，因此失败不改变候选账户或共享权威账户状态。
- 成功时通过 `Arc::make_mut` 对候选账户执行 COW 写回现金。测试验证候选现金增加、原权威账户现金不变、持仓不变且状态解除共享；失败路径测试覆盖非正值及溢出时状态不变。
- 新错误携带被拒绝金额；没有静默忽略失败的路径。未发现账户账务或其他 A 股交易制度语义变化。

## 验证限定

动态验证证据为 `.tmp/company-system/session-actions/fresh-0.log`（5/5）；复核人没有重跑 Cargo，也没有检查或签核调用方资金流程。
