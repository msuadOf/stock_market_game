# NPC 观察概率的数值精度

## 真实故障与根因

原 5 证券、26 NPC、seed `666959854` 的真实 `ProtocolSession` 日终候选已通过 Native 恢复及严格 archive 校验，但 normal WASM Worker 恢复时拒绝 NPC 7 的基础观察概率：Native 为 `0.03979332740595576`，WASM 重建为 `0.03979332740595565`。两个真实 Browser 分片均未到 IndexedDB 写入。日志为 `.tmp/checklist-wave4/real-browser-wasm-day-end-current.log`，独立证据边界见 `agents/shared-market-accounts/real-browser-wasm-validation.md`。

`daily_observations_to_tick_probability` 的模型是 `1 − exp(−observations_per_day / ticks_per_day)`。原计算先求接近 1 的指数值再相减，将指数末位差异放大为概率的相对误差；极小的正频率还会归零。改用 `−expm1(−observations_per_day / ticks_per_day)`，数学模型及个体参数不变，不扩大恢复容差、不引入存档兼容或新依赖。

## TDD 与独立核算

新增三个普通测试分别覆盖常规概率、稀少观察和完整 u64 tick 域。参考值先以实际 f64 除法结果为输入，使用 Python `Decimal` 80 位精度计算指数再舍入为最近 f64；独立 reviewer 另行复算全部位模式一致。测试要求真实正概率且距参考最多 1 ULP，不用相减结果作为自己的参考。

root 统一编译成功，耗时 53.72 秒，产物来自 `.tmp/checklist-wave4/observation-probability-red-build.jsonl`。实际 `--list` 确认三个 case 后，使用外部 10000ms deadline、`RAYON_NUM_THREADS=16`、`--test-threads=3` 并行执行，三个均真实失败：常规值偏差超过 1 ULP、稀少值精度不足、极小正值归零。日志为 `.tmp/checklist-wave4/observation-probability-red.log`；底层退出码 101，外部 runner 报退出码 1，不能把包装 shell 的成功输出当成测试绿色。

随后 root 在 `host38` 中统一编译当前源码，耗时 55.59 秒；真实产物清单为 `.tmp/checklist-wave4/host38-binaries.json`。三个 case 经 `--list` 确认后以相同 10000ms deadline、Rayon 16、测试线程 3 执行，实际 3 通过、0 失败，耗时 0.00 秒，日志为 `.tmp/checklist-wave4/host38-observation-probability-green.log`。这闭合了上述数值计算的真实红绿，不替代跨平台存档验收。

## 验收边界

生产实现只替换同一公式的数值求值方式。Rust 三项已实际绿色；Native release 重新编译耗时 38.33 秒，原 5 证券／26 NPC／seed `666959854` 的真实 Protocol producer 完成两个日终、恢复及重保存深等值，两分支后续三帧的真实 NPC 受理和 cursor 检查也通过。新 fixture SHA 为 `5457be5b68eba83576a0a87f4f54b8123d119a252543b7e49721924977a5fcb1`。

normal WASM 正规重建约 63 秒，包校验无 verification 或私有诊断导出；SHA 为 `36210175fa7a3152dea84ff407f6be3f53813aef2cf2653b8c359105aef9a58f`。两个独立 Browser Worker／Rayon 2 分片执行真实恢复、保存、严格日终候选校验、IndexedDB 存取及再次恢复，快照等值且两类 SHA 前后一致；整命令 6.93 秒、命令与 case 均 10000ms deadline。日志为 `.tmp/checklist-wave4/real-browser-wasm-probability-current.log`，其中 3 passed 包含父 test，实际场景是两个分片。

非作者亲读源码、三红三绿及上述真实跨层日志，直接核对文件 SHA 后通过原 NPC 7 故障的限定门禁。旧失败证据不删除；不会借此宣称所有 `exp`、`powf` 或随机采样在各平台逐字节相同，也不扩大为自动日终、全宿主或完整回归。后续遵照用户最新要求，只做简单单元测试及独立源码完成度复核，不启动复杂回归。独立复核见 [精度复核](observation-probability-precision-review.md)。
