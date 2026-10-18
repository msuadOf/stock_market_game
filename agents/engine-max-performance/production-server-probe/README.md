# 正式 Cargo 配置的真实 server 探针

此普通 binary 调用真实 `SessionManager` / `SessionActor Fastest`，保留逐帧 tick 连续校验和休市自然日推进。
不包含 HTTP/WS 序列化或 frontend。源码必须与 `apps/server/examples/server_max_speed_probe.rs` 逐字节一致。

Cargo example 会加载 server 的 dev-dependencies，使 engine 开启 `verification-harness`、Tokio 开启
`test-util`。本工具位于独立 workspace，只有普通 dependencies，并沿用根 workspace 的依赖版本。
不能把移除验证工具 feature 当作删除业务守恒、费用、撮合或失败原子性门禁：那些生产检查保持执行。

所有运行必须从仓库工作区根目录开始。先核对源文件与锁文件；根 Cargo.lock 变化时，复制根 lock，
使用 `cargo metadata --offline` 剪裁包图，再确认除本地 probe package 外，没有引入根 lock 之外的依赖。
依赖不一致时停止，不得把新依赖版本与 feature 变化混进一次控制测量。

```sh
timeout --signal=TERM --kill-after=1s 299s cargo build \
  --manifest-path agents/engine-max-performance/production-server-probe/Cargo.toml \
  --offline --locked --release --bin production-server-max-probe -j 16 \
  --target-dir .tmp/production-probe-target --message-format=json \
  > agents/engine-max-performance/production-probe-controlled-build.jsonl \
  2> agents/engine-max-performance/production-probe-controlled-build.log
python3 agents/engine-max-performance/production-server-probe/verify-build.py \
  agents/engine-max-performance/production-probe-controlled-build.jsonl
```

必须确认构建进程 `exit0` 后才能运行 verifier，不能仅根据 `Compiling`、`build-finished` 日志推断。
verifier 核对实际 artifact 的 engine/server features 为空、Tokio 不含 `test-util`、target kind 为 `bin`、
release opt-level 为3、源字节与当前 probe 一致、依赖 name/version/source/checksum 属于根 lock。
源文件或配置比 binary 更新时显式拒绝旧日志。输出 binary/source SHA 作为测量绑定。

正式测量不与构建、其他基准重叠，固定实际并发参数并记录 CPU/线程利用率：

```sh
timeout --signal=TERM --kill-after=1s 299s python3 \
  agents/engine-max-performance/cpu-probe.py server-production-normal-16 \
  16 0-15 .tmp/production-probe-target/release/production-server-max-probe
```

示例 label 使用前须检查既有日志，后续测量使用新的 label 保留全部样本。
此工具不声明 1000x 达标、满核、WASM 或 frontend 验收完成。
