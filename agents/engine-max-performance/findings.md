# Rust / Browser max 性能排查

## 目标与隔离

- 工作区：`.worktree/engine-max-performance`，分支 `fix/engine-max-performance`，基线 `a6001494`。主工作区 `main` 的未提交修改未复制、未修改。
- 目标：默认 5 股、20,007 NPC、16 核，完整权威市场 tick 达到 1000x 以上；一个市场 tick = 一秒。不得减少 NPC、跳过撮合/校验/个人观察、改变 A 股规则或虚报时间单位。
- 已读根 AGENTS、principles、testing、architecture、open-questions、Git 日常工作流及 ADR-0002/0008/0017/0018/0019。未改变交易制度，依据沿用现行正式交易规则文档。
- 编译显式 `-j16`，各构建/长性能测量均进程外 `timeout`，TERM 299 秒、最后 1 秒 KILL，最大共享期限 300 秒。普通短测单独 10 秒。

## 基线证据

- `max_speed_probe` 使用 Web DEFAULT_SETUP、seed 42、原有完整 `step_with_phase_timing`，包含必要自然日推进。初始化不计入市场测量，日内自然日工作计入。
- `baseline-native-16.json`：CPU affinity 0–15、Rayon 16，前 1000 tick 5.798 秒 / 172.459x；5953 事件。
- `baseline-native-1.json`：独立 CPU 16、Rayon 1，前 1000 tick 9.839 秒 / 101.640x；5953 事件。同事件数量不等于不同 worker 的自由运行整局相同。
- `baseline-native-3000.json`：CPU affinity 32–47、Rayon 16，含连续竞价的 3000 tick 38.362 秒 / 78.202x；25495 事件。短开盘窗口不能替代连续竞价或日终验收。
- `baseline-browser.json`：Vite 5187、真实页面成功启动后新建默认 WorkerHost，CPU affinity 0–15、明确 threadCount=16、Chromium hardwareConcurrency=16。市场 warmup 后观测 347 个实际提交帧 / 15.977 秒，21.719x，无 fatal，crossOriginIsolated=true。此样本仍只覆盖开盘竞价，未满足目标。
- Browser 初次失败原因是手工 wasm-bindgen 输出未生成 package.json，Vite 无法解析 Rayon worker 的目录 import。已补齐生成产物包元数据；未改产品代码。
- WASM 冷编译 2m34s，通过；wasm-pack 的相对 CARGO_TARGET_DIR 被 Cargo 在 crate 目录解释，随后 CLI 在根目录找不到 artifact。直接调用同版本缓存的 wasm-bindgen 生成 bindings，并通过 check-wasm-threading。后续必须使用绝对 target-dir。
- 系统 perf_event_paranoid=4，perf record 被拒绝；未更改系统权限。原始原因保存在 profile-native.err。

## 已定位成本

- max 每个 tick 使用嵌套 setTimeout(0)，存在浏览器最小 timer 间隔；必须保留逐 task 接收玩家 ingress 的能力。
- Rust 每 tick 无条件遍历全部账户检查 strategy，然后 AccountBook 的页级策略缓存再次检查。
- DecisionResourceSnapshot 每 tick 对全部账户、全部已持有股票校验资源和 equity，20,007 户最多约 100k position 检查，即使大多数页完全未改变。
- live ledger 在 shadow、股票输入、receipt aggregation、precommit 等边界多次 clone、完整验证和 rebase；成本随存续委托增长。
- 临时 clone 细分探针已移除。前 1000 tick 中 ledger clone 总 167.893ms、attention heap clone 30.782ms、market clone 18.432ms、ingress cursor clone 14.522ms。它们只是固定 clone 成本，不能据此省略全局守恒校验。
- 一次性 Luna high 只读分析支持上述定位；该分析不是实施后独立 diff 复核。

## 未完成

- 资源缓存正在 TDD 实施，尚未取得行为红灯/绿灯闭环。
- Browser task scheduler、ledger 重复全量工作及更大核心数扩展仍待优化。
- 16 核 >=1000x 尚未达到；全交易日、长期稳态、真实 UI 消费、语义回归和独立复核均未完成。不得宣称任务完成。

## 第一批缓存进展

- 新增页缓存短测先失败：两次相同输入校验计数为4，预期2（resource-cache-red.log）；实现后绿灯。
- cache key 包含不可变 page identity（mutation/clone 清缓存）、完整价格 map、本页现金及卖出预留。未知账户/股票引用仍逐次检查；失败不写成功缓存。
- 最新短测：AccountBook 4项、DecisionResourceSnapshot 11项、失败/poison 6项全部通过，各命令不超过10秒。新增价格溢出测试先预置成功缓存，保证旧成功结果不掩盖后来价格溢出。
- 16核3000 tick 在旧 pool.install 探针下为81.47x和83.15x；1核1000 tick为218.31x。不能把不同窗口结果相互比较。
- 修正 probe 到 global Rayon pool、调用线程 coordinator 后，16核3000 tick为76.89x；尚未重新在统一入口生成无缓存baseline，因此不声称该结果有可配对优化收益。
- `time -v` 显示16 worker实际仅约225–233% CPU，系统耗时23–28秒、数百万次context switches。各级cgroup的cpu.max为max，未观察到CPU quota限流；仍须进一步定位调度开销。
- 独立 Luna high 复核记录见 resource-cache-review.md，未发现当前语义/失效缺陷。通用closure尚只有一个生产validator；如扩充调用方必须重新设计失效契约。
- diff --check 与本次文件 rustfmt --check 通过。没有跑完整回归，不宣称全局green。

## 第一批缓存进展

- 新增页缓存短测先失败：两次相同输入校验计数为4，预期2（resource-cache-red.log）；实现后绿灯。
- cache key 包含不可变 page identity（mutation/clone 清缓存）、完整价格 map、本页现金及卖出预留。未知账户/股票引用仍逐次检查；失败不写成功缓存。
- 最新短测：AccountBook 4项、DecisionResourceSnapshot 11项、失败/poison 6项全部通过，各命令不超过10秒。新增价格溢出测试先预置成功缓存，保证旧成功结果不掩盖后来价格溢出。
- 16核3000 tick 在旧 pool.install 探针下为81.47x和83.15x；1核1000 tick为218.31x。不能把不同窗口结果相互比较。
- 修正 probe 到 global Rayon pool、调用线程 coordinator 后，16核3000 tick为76.89x；尚未重新在统一入口生成无缓存baseline，因此不声称该结果有可配对优化收益。
- `time -v` 显示16 worker实际仅约225–233% CPU，系统耗时23–28秒、数百万次context switches。各级cgroup的cpu.max为max，未观察到CPU quota限流；仍须进一步定位调度开销。
- 独立 Luna high 复核记录见 resource-cache-review.md，未发现当前语义/失效缺陷。通用closure尚只有一个生产validator；如扩充调用方必须重新设计失效契约。
- diff --check 与本次文件 rustfmt --check 通过。没有跑完整回归，不宣称全局green。
