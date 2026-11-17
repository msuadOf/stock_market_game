# 真实前端 Server Native / Browser WASM 性能对照

## 基准：2026-11-17

以 `192773d7d5bd18f5dc83174a3f3977ea07933161` 的 release 产品为被测对象；本提交不修改产品代码。两端在同一 Dot Linux 云电脑上，共用 CPU 亲和集 0–8 和同版 Chrome Headless Shell 153.0.8010.12，使用实际生产页面的移动端个股分时视图（390×1180），分别连接 Native HTTP/WebSocket Push 和本地 shared-memory WASM。固定 seed 42、两只股票（600101 沪市、002156 深市）、70 个 NPC（64 retail、4 inst、2 hot）、retail `arrival_rate = 0.3`、起始日期 2030-01-04、60,000 tick/日及 `every_tick`。所有有效采样均在同一模拟交易日内。串行交替运行三对，避免同一 CPU 组争用；每端测 10 秒 1×、5 秒 MAX。

| 指标（三组中位数） | Server Native | Browser WASM |
| --- | ---: | ---: |
| 1× 权威速度摘样（tick/s） | 0.996 | 0.998 |
| MAX 权威速度摘样（tick/s） | 4,744.82 | 343.22 |
| MAX / 1×（各自配对倍率的中位数） | 4,762.22× | 343.80× |
| MAX 界面游标推进（tick/s） | 4,082.71 | 333.72 |
| MAX `requestAnimationFrame` 回调次数/s | 51.91 | 59.70 |
| MAX 回调间隔 P95（ms） | 33.33 | 16.67 |
| MAX 页面主线程 `TaskDuration`（ms/s） | 899.39 | 306.53 |

按采样顺序，MAX 权威速度 Native 为 4,830.47、4,744.82、4,721.40 tick/s；WASM 为 343.22、273.40、354.16 tick/s。权威值来自窗口内周期读取的引擎 speed metrics 片段，不是完整窗口的起止权威游标；汇总采用覆盖片段的总 tick / 总时间。界面游标是 5 秒窗口的 DOM tick 差，不是逐 tick 绘制；回调数也不是已绘图帧数。MAX 的两端会走到同日的不同 tick 区间，不能当作锁定相同输入轨迹的每 tick CPU 成本对照。不可把本轮与不同负载、无渲染的运行直接相减作前端开销。

## 独立诊断

- 在相同生产制品和 WASM 路径下，仅修改测试浏览器发送的初始化参数 `waitSettings.fineUs`（500 → 100 → 100 → 500 µs），MAX 权威速度依次为 352.68、446.09、421.03、365.75 tick/s，两种设置的中位数为 359.21 和 433.56 tick/s（+20.7%）。该参数同时可能作用于协调者和 worker 空闲轮询，不能仅归因于协调者某一个等待点。Native 用通知可唤醒的 `recv_timeout`，WASM 使用 `try_recv` 加分段睡眠。对照说明轮询是损耗之一，不是其解释了两端差值的 21%。
- 独立插桩的浏览器构建在 `market_runner_turn` ABI 调用两端读时钟，并累计到下次进入的间隔。四轮中调用占采样 wall time 99.57%–99.62%，轮次之间均值约 60–66 µs。调用内含 Rust 计算、Worker 等待和 ABI 成本，不等于 CPU 利用率。未分别归因撮合、NPC、队列和组包，也没有原生端的同口径 turn trace。插桩原始结果不参与正式速度数字。

## 复现正式基准

在仓库根目录准备 `cargo build --locked --release -p server --bin server --features web-ui` 以及 `apps/web/dist` 的正式生产前端（锁定依赖及 release shared-memory WASM，见仓库构建说明）。实测 Native SHA-256 为 `67f8bdf9622dc3b687508c9ac9745c8cb1d39da8210866db15ab643d5e10d4ac`，页面实际加载的 WASM SHA-256 为 `634ba9cf9c39a5161526210b6ff86d83053c5c72f83b5390d8b698d0a9476f0d`。本提交只增加测试资料，从本提交构建时二进制可能因元数据变化而不同。

```sh
export BENCH_CHROMIUM=/absolute/path/to/chrome-headless-shell
node scripts/run-long-validation.mjs 300000 -- taskset -c 0-8 \
  node agents/real-ui-performance/run-pairs.mjs
python3 agents/real-ui-performance/summarize.py agents/real-ui-performance/run-实际时间戳
```

每个样本只启动 loopback 测试服务，建独立临时 SQLite 和浏览器上下文，不使用真实账号或存档。测试脚本在会话创建前设置 fixture，并校验实际初始化。它使用 React 内部状态读取探针，绑定此版本 UI，不是公共接口或通用 CI 测试。可用 `BENCH_BASE_PORT` 指定一组空闲端口；采集输出、本地环境日志和插桩副本不要加入 Git。
