# Simple 年化 growth leaf 独立复核

## 范围与结论

复核对象：`packages/engine/src/company/simple/growth.rs` 全文件、`packages/engine/Cargo.toml` 中 `num-bigint = "0.4.8"` 依赖 hunk，以及 ADR-0036。只做静态数学与工程审查；未运行 Cargo，也未修改实现。

结论：年化基点到 1e9 定点 12 次根的表达式与 ADR-0036 一致；整数根 midpoint 比较实现最近值及 half-even 判定正确。未发现 A 股交易语义或金额单位漂移。发现一个应该补齐的金额边界测试，已告知实施者；这不构成已验证通过的完整门禁。

## 数学与范围核对

- `annual_scaled = (10000 + annual_growth_bp) * 100000`，即 `(1 + 年化增长率) * 1e9`；`annual_scaled^months * 1e9^(12-months)` 的 floor 12 次根给出 `annual_scaled^(months/12)` 的定点 floor。
- midpoint 比较将 `(floor + 1/2)^12` 与真实目标比较：两边乘以 `2^12` 后正是代码中的 `((2*floor+1)^12)` 与 `target << 12`。相等时以 floor 奇偶决定是否进位，实现 ties-to-even。
- `months` 限制为 1..=12 与 ADR 规定的本轮月跨度相符；年增长低于 -100% 被拒绝，恰为 -100% 时结果根为零。基点加法在 `i64` 中进行，`i32::MAX` 上界也不会溢出。
- 最大合法年化输入构造的整数幂约百余位，`BigUint` 用量固定且很小；大整数仅用于根比较，因子 compose 与金额应用使用 checked 整数运算。当前未见有意义的性能风险。
- compose 的 `u64 × u64` 乘积可容纳于 `u128`，half-even 余数比较不会溢出。金额商余分解没有先乘完整金额与因子；`unsigned_abs()` 覆盖 `i128::MIN`，唯一可表达的负边界通过专门分支还原。
- `AccountingAmount` 仍以 `i128` 分存储；`apply` 的最终舍入仅作用于分（cents），未引入浮点、股数或 wire/Money 类型影响。

## 发现

- **应补边界用例：`AccountingAmount::MIN`。** 当前测试以 `AccountingAmount::MAX` 验证单位因子及正向溢出，但没有覆盖实现中特设的 `total == 1 << 127` 负边界分支。建议最少断言单位因子应用于 `MIN` 仍为 `MIN`；再断言对 `MIN` 应用大于 1 的因子显式报错。该遗漏涉及金额范围的防回归，不改变实现判断。已发给实施者，等待其修复与复核。

## 大 A 语义、必要性与范围

该 leaf 只把明确以年化基点表达的基本面增长换算为自然月段的复利因子；`AccountingAmount` 的人民币分单位没有变化。未触及交易所制度、证券分类、委托、成交或账户 `Money`，无需外部交易规则依据。`num-bigint` 仅用于 ADR 明确授权的整数根比较，且为 engine 已有锁定版本的直接依赖；用途与改动范围相称。
