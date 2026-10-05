# `CompanySpec.issued_shares` wire 修复记录

## 范围

`CompanySpec.issued_shares` 是已发行普通股总股数，Rust 内部仍为 `u64`，仅调整 JSON wire 表示为规范十进制字符串。Web 端将它作为 `string` 校验，避免 JavaScript `number` 对超过 `2^53 - 1` 的股数舍入。股数含义、范围（`1..=u64::MAX`）与业务算术未改变。

## 修改

- Rust `CompanySpec.issued_shares` 复用现有 `canonical_u64_decimal` codec 编码与解析，并在字段局部反序列化时拒绝零；共通 codec 继续允许零。
- Web `CompanySpec` parser 使用 `decimal` 并拒绝零；`decimal` 的正则收紧为规范形式 `^(0|[1-9]\d*)$`，拒绝前导零。
- Web 发行人与股票总股本交叉核验继续使用 `BigInt`，不转换成 `number`。
- 新增 Rust/Web 边界测试，覆盖 `u64::MAX` 无损 round-trip、零、前导零、超范围和数字 JSON 拒绝；调整直接构造 `CompanySpec` 的 Web fixture。

## 语义与依据

本改动只修复现有已发行普通股总股数的数据精度与规范编码，不引入交易制度或股数计算规则变化。领域语义沿用 [ADR-0035](../../docs/decisions/0035-company-system-simple-fundamentals.md) 与 `CompanySpec` 的既有定义。

## 验证与复核

Rust 与 Node 测试仅编写，待根 agent 统一运行；本记录不声称测试已通过。依仓库门禁，完整 diff 仍需由未参与实现的 reviewer 独立复核。

## 观察到的相邻范围

Rust 通用 `canonical_u64_decimal` codec 已拒绝前导零，本次不扩大修改该 codec。Web `decimal` 是多处存档字段共用的 primitive，因此规范 regex 调整影响全部使用该 parser 的 u64 字符串字段；其行为与 Rust canonical codec 一致。
