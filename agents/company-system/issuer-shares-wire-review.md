# `CompanySpec.issued_shares` wire 独立复核

## 范围与结论

复核对象为 `CompanySpec.issued_shares` 的 Rust serde、Web `CompanySpec` 类型与 parser、共用 `decimal` primitive、对应边界测试，以及 `agents/company-system/issuer-shares-wire.md` 记录。未修改实现、Cargo 或 index。

初审发现 P1：Rust `CompanySpec` 反序列化会接受 `"0"`。实现者增加 `positive_canonical_u64_decimal` 局部 serde module 后，增量复核关闭该发现：序列化委托通用 canonical codec；反序列化先使用同一 codec，再拒绝零值。通用 `canonical_u64_decimal` 未改，因此其他 `u64` 字段契约不受正数限制影响。Rust 边界测试断言最大值 round-trip、零、前导零、越界与 JSON number；Rust targeted test 在 `.tmp/checklist-wave4/host70-simple-short.log` 报 `1 passed`，Node 两项 parser/primitive 测试在 `.tmp/checklist-wave4/host69-shares-web.log` 报 `2 passed, 0 failed`。已亲读日志，动态验证缺口关闭。

## 复核要点

- 股数语义仍是 `issued_shares` 已发行普通股总股数，Rust 内部仍为 `u64`；JSON 字符串编码避免 JavaScript `number` 对大整数舍入。Web 发行人与股票配置交叉比较继续以 `BigInt` 比较字符串，没有转成 `number`。
- Rust codec 接受 `"0"` 或无前导零的 ASCII 数字串，溢出由 `parse::<u64>()` 拒绝；CompanySpec 局部 guard 将允许范围收为正数。Web `decimal` 的 `^(0|[1-9]\\d*)$` 与范围检查对齐 canonical 编码。其共用影响是拒绝此前可接受的前导零写法；这与 Rust canonical codec 既有契约一致。
- Web 测试覆盖 `u64::MAX`、JSON number、零、前导零、越界和小数形式；Rust 对应边界测试现已与局部拒零实现一致。
- 改动没有扩张旧 mask-group 配置或交易规则；不改变 A 股股本含义、股份算术或撮合语义。依据沿用 `CompanySpec` 既有定义及 ADR-0035，无需新增交易制度依据。
