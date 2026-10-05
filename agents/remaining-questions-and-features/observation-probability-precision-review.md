# NPC 观察概率数值精度独立复核

## 范围与结论

复核者未参与实现。已完整阅读 `strategy/sampling.rs`、`strategy/factory.rs`、
`session/attention.rs`，检查 `session.rs` 的 NPC 构造、attention 恢复与
`json_canonical_f64` 路径，并对照 AGENTS、工程原则、开放问题及相关 ADR。
本次限于基础观察概率 `1 - exp(-observations_per_day / ticks_per_day)` 的数值计算。

以 `-exp_m1(-observations_per_day / ticks_per_day)` 替代相减形式是同一数学模型的
数值稳定实现，不更改观察次数、策略分布、RNG 消费、账户资金、A 股 T+1、申报单位
或撮合。该概率属于明确的游戏行为假设，不冒称交易所规定的投资者行为；无需新增
交易制度依据。无新依赖、存档字段、版本兼容或默认修补。为使合法日终存档能够在
同一引擎的 Native 与 WASM 宿主恢复，此根修有必要且范围最小。

当前限定 diff 仅含一行 `exp_m1` 替换及三个准确度测试。静态审查、参考常量核验
与 Rust 短测红绿门禁通过。fresh Native 日终候选与 normal WASM 的原真实 Browser
双分片已通过，下述哈希、实际日志与制品亲读核验后，原 NPC 7 概率恢复故障的
限定门禁关闭；不把此结论扩张为所有输入、数学函数或完整宿主矩阵确定性保证。

## 已有真实故障

`real-browser-wasm-validation.md` 与
`.tmp/checklist-wave4/real-browser-wasm-day-end-current.log` 记录真实 normal WASM
拒绝 Native ProtocolSession 日终候选：NPC 7 保存概率
`0.03979332740595576`，WASM 重建概率 `0.03979332740595565`，超过原来的
`scale * f64::EPSILON * 4` 恢复守卫。该路径未到 IndexedDB 写入，因此不声称 IDB
成功。`session.rs` 恢复守卫及 JSON canonical 路径均未因本根修放宽。

`exp(-x)` 接近 1 时，其末位误差经 `1 - exp(-x)` 放大为输出的相对误差；极小
正数更可能直接得到零。`exp_m1` 直接计算 `exp(-x) - 1`，避免该消去。

## 三个准确度测试与独立复算

测试不是用待改实现自算期望值。复核者独立用 Python `Decimal`，80 位精度，先
执行与 Rust 相同的 f64 输入除法，然后用 `Decimal.from_float` 精确导入该商，计算
`1 - exp(-x)`，最后取最近 f64 的位模式。这里验证的是函数实际 f64 运算输入，
不是把十进制 `0.1`、`2.4` 或 `u64::MAX` 当成完全精确的实数。

| 输入 observations／ticks | 参考 f64 位模式 | 原相减实现的独立本机计算 |
|---|---|---|
| 2.4／60 | `0x3fa4136818ff472b` | `0.03921056084767682`，超过一 ULP |
| 0.1／100000000 | `0x3e112e0be801f1d9` | `9.999999717180685e-10`，显著相对精度损失 |
| 0.1／u64::MAX | `0x3bb999999999999a` | 零，违反有效正概率 |

三常量与测试完全一致。各测试同时要求有限、严格正且不超过 1，并要求与参考最多
相差一 ULP；这能检出消去，不通过放宽恢复守卫来掩盖错误。稀疏观察与 tick 数上界
覆盖原式最不稳定的有效输入；工厂已显式拒绝零 tick，生产观察率由正的有界风格
采样生成，因此没有为内部助手引入无关的配置校验或负率语义。

复核者已亲读 `.tmp/checklist-wave4/observation-probability-red.log`：旧公式实际运行
三个 case，均失败；中等概率和稀疏概率准确度失败、tick 上界正概率断言失败，
失败值与独立复算相符，命令 exit 101，并非编译失败或零 case 的假红。
随后亲读 `.tmp/checklist-wave4/host38-observation-probability-green.log`：同名三个
case 全部通过，0 ignored，0.00 秒；未改 golden 或弱化断言。Root 统一完成真实
host38 编译，本复核者没有额外启动 Cargo 或修改实现。

## 原跨宿主故障的真实复验

已亲读 `.tmp/checklist-wave4/real-browser-wasm-probability-current.log`、对应 Browser
验证记录及非作者工具复核记录，并直接计算当前实际文件 SHA256，与日志一致：

- Native ProtocolSession 完整日终 fixture：
  `5457be5b68eba83576a0a87f4f54b8123d119a252543b7e49721924977a5fcb1`。
- fresh normal WASM：
  `36210175fa7a3152dea84ff407f6be3f53813aef2cf2653b8c359105aef9a58f`。

原场景两个隔离 Chromium context 并行、各 Worker Rayon 2，约 5.72／5.68 秒完成，
整命令约 6.93 秒，处于 10000ms 进程树与 case 上限内。日志实际记录
`committed=true`、`selected=true`、`sameSnapshot=true`，tick 120、seq 1473、
settledDate 2030-01-08、两个槽；总 pass 3 包含父测试，独立场景是两个而非三个。
该工具真实执行 normal Worker `restore_json`、公共 baseline、snapshot/save、严格
日终候选校验、IndexedDB save/list/copy/select/load、第二次 WASM restore/snapshot
等值，并守卫前后 SHA 及 normal export/capability。原 NPC 7 恢复错误没有重现，
此次确实到达并提交 IndexedDB，不再停留于此前未写入的失败路径。

因此认可原数值消去故障的限定修复。此 Browser 场景未推进新的自然日，不能据此
宣称新自动 EOD 生产、所有 Host、多人 Server E2E 或全部 checklist 已通过；旧红日志
仍作为根因与 TDD 证据保留。

## 必须保留的验证边界

- Rust 三个 case 的原公式失败、改后通过已核验；Python 独立复算用于审查 golden
  常量，而非替代该真实红绿。
- 原真实候选的 fresh Native→normal WASM／IndexedDB／二次恢复路径已绑定新 SHA
  通过，未修改 JSON 内概率、删状态、换 seed 或放宽断言；不推广未覆盖的运行路径。
- 单测只证明所列准确度样本，真实宿主短验证只证明其覆盖的实际候选，不证明所有
  平台、输入与 math 实现逐字节一致。初始现金采样和 split tail 的 `exp`／`powf`、
  几何等待的 `ln`／`ln_1p` 没有因此自动核销。
- 本复核不支持同 seed 在不同自由并发受理轨迹下整局字节相等的扩大表述，也不以
  数值稳定修复替代最终完整回归。
