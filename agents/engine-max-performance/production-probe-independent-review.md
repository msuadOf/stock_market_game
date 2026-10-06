# 正式 server probe 独立复核

复核范围仅限 `production-server-probe/` 的 `Cargo.toml`、`Cargo.lock`、`src/main.rs`、`verify-build.py`、`README.md`，以及指定的构建与测量证据。未启动构建或基准；按要求执行了一次 `verify-build.py`。

## 结论

1. **大 A 语义与基准口径可靠。** 这是生产 `SessionManager` / `SessionActor Fastest` 的性能探针，不改交易规则、金额或时间单位。输入仍为 5 股、20,007 NPC（20,000 retail、5 inst、2 hot）、seed 42；每次接收并逐帧核验 3,000 个连续 tick，速度单位是 tick/s，Rayon 为 16、Tokio 为 2。`main.rs` 与既有 example 逐字节相同，SHA-256 为 `aff0e24f66e5d0c50f5235cc59e52cab33973cfb6d42de3228f342cbe43471c3`。旧 example 会加载 `server` 的 dev-dependencies，启用 `verification-harness` / Tokio `test-util`；文档准确区分了旧测量的 actor 路径与其编译配置，也明确说明移除这些 feature 不代表删除正常业务 gate。

2. **改动范围必要且较小。** 探针是独立普通 binary，只声明现有 workspace 依赖，没有 dev-dependencies。独立比较源码字节相等；probe lock 相对根 lock 仅多本地 `production-server-max-probe 0.0.0`，共同依赖的 name/version/source/checksum 一致。verifier 实际执行成功，报告 engine/server features 为空、Tokio 不含 `test-util`、target 为普通 bin、release `opt-level=3`、构建完成成功，且当前源码/配置 mtime 不晚于 binary。最终 proof 的 SHA 与 cpu-probe 两份样本记录的 SHA 一致；样本为 159.7196x 与 163.2210x，均完成 3,000 tick。

3. **未发现交易语义漂移、遗漏的基准边界或不必要复杂度。** 唯一可选改进：verifier 校验了 artifact 的 `src_path`，但没有要求 `executable` 位于 README 命令指定的 `.tmp/production-probe-target/release/`。若日志来自另一 target-dir，verifier 仍可能接受它；可将 executable 解析后与预期路径比较，使 binary 路径也具备明确绑定。当前指定日志及 proof 路径正确，此项不影响本次样本结论。
