# Linux Desktop 图形验收与三端 MAX 测量

## 图形会话前置条件

Dot 云机的 Deb 只解包到工作区，没有把依赖写入系统目录。GTK/WebKit 链接库可解析；Xvfb TCP 连接使用每次独立、随后删除的授权凭据。Debian WebKit 的三个进程路径在编译时固化，`locating-webkit.c` 只把这三个路径映射至同一套解包二进制，非匹配调用透传。**不关闭 WebKit sandbox，不改系统配置。** 如果产品因 WebView 其他功能受阻，要报告实际错误，不能用无窗口 actor 样本补 Desktop。

预检证实真实 MiniBrowser 的 X11 窗口标题由文件 fixture 的 JavaScript 更新，并出现独立 WebKitNetworkProcess / WebKitWebProcess；这不等于产品通过。参考启动：

```sh
D="$PWD/agents/shared-market-runner/libos-desktop-fixture"
S="$(realpath ../.shared-runner-tools/desktop/root)"
export LOCAL_DESKTOP_SYSROOT="$S" LOCAL_DESKTOP_REPORT="$ABS_FRESH_RUN_DIR"
node scripts/run-long-validation.mjs 90000 -- bash "$D/with-xvfb.sh" "$TAURI_DESKTOP_BINARY"
```

构建前验证 `apps/web/dist` 的 WASM 制品来源和版本。Tauri 嵌入同一源码生成的前端，使用默认 `custom-protocol`；不以挂载 dev server 替代。设置 `PKG_CONFIG_SYSROOT_DIR="$S"`、`PKG_CONFIG_LIBDIR="$S/usr/lib/x86_64-linux-gnu/pkgconfig:$S/usr/share/pkgconfig"` 与 `LD_LIBRARY_PATH="$S/usr/lib/x86_64-linux-gnu"`。Native Server 用 release + `web-ui`，以独立临时目录、loopback、非用户存档服务；Browser 独立版由带跨域隔离头的本地静态 HTTP server 提供 release `dist`。固定 CPU set 和 worker 预算；构建与计时不可并发。

Desktop 自动化可用真实 WebKit Inspector（专用构建启用且记录 `tauri/devtools`，只绑定 loopback）；采样前先证明实际 WebView 能自动化且 authority IPC 给出数据。若自动化受阻，实际窗口完成操作和视觉检查后只记录 smoke；不把 `tauri::test::mock_app`、DEV mode 或无窗口 `production_actor_performance` 记录成 Desktop UI 样本。发布物不启用 devtools。

当前已经通过单页 WebKit preflight 的真实 `Runtime.evaluate`；接线使用 WebKit 的 `Target.sendMessageToTarget`，不是 Chrome CDP。构建专用 Tauri 制品后，使用同一图形入口运行已有 app UI 和原生日程：

```sh
# 由统筹者在源码冻结后启动，与其他重型编译合计不超过可用并发。
cargo +1.96.1 build --locked --offline --release -p stock-market-game \
  --bin stock-market-game --features tauri/devtools --jobs "$BUILD_JOBS"
node scripts/run-long-validation.mjs 90000 -- bash "$D/with-xvfb.sh" \
  bash "$D/run-desktop-smoke.sh" "$TAURI_DESKTOP_BINARY"
```

脚本确认是嵌入式真实 Tauri WebView、选本地、进入并推进，采 1× 和三个 MAX 窗，分别记录权威采样和 UI 净推进。它用实际产品默认开局（随机 seed），只能作为单端 smoke。也可给 `run-desktop-smoke.sh` 第二实参：`stalled`、`fixed`、`max` 或 `all`。前三个用 `desktop-controlled.mjs`，从实际嵌入 WebView 本地 IPC 启动，共用下述 setup/seed，记录连续观测与权威吞吐；它们不经过 React 渲染，不能标成 UI 吞吐。`all` 先做三组 IPC，再做真实 UI smoke；需要首次报告中分别录制两组可核验原始输出。

## 公共负载与验证

三条真实产品路径是 Chromium → release shared-memory WASM、Linux WebKitGTK Desktop → Tauri IPC、Chromium → Native Server HTTP + WS。全部从相同的种子和 setup 开始。

| 场景 | 负载 | 验证 |
| --- | --- | --- |
| 交易与日结 | `scripts/fixtures/libos-product-scenario.mjs` 种子 42、沪深两个 Desk、70 个 NPC 周五开局 | 双交易所真实成交，日期相同，日 K 保留、分钟数据不跨交易日 |
| 停收跨交易日 | 同 setup，12×，停止 ACK / 观察读取但不停止控制 | 耗尽真实观察窗口，观察 tick 停滞，权威到周一后仍完成新 tick，刷新后只显示当天分时 |
| 稳态 MAX | 固定种子 42、同一股票目录和 NPC 数量，在报告里固定对应 setup | 固定稳态时间窗；超过或跳过的游标不得作为连续送达计入 |
| UI 端到端 | 固定 1200×800、同一股票和分时图，前景可见 | UI tick 和图表均推进，报告画面更新间隔与观察缺口 |

通过受控交易 fixture / 产品接口导入同一 setup，记录 setup SHA、seed 与第一个 authority tick。浏览器可注入评估 harness（真实 release WASM 与 worker）；Desktop 使用实际 WebView 的现有命令 `create_session`、`engine_baseline`、`set_speed`、`resume_session`、`speed_metrics`；Native 使用已认证 REST/WS 对应契约。如果一端无法锁定 setup/seed，只记录 smoke，不标记三端性能为可比较。Browser 正式 UI 跑静态 `dist`；Vite fixture 仅供控制命令和定位交付回归。

## 采样指标

1. 每端先跑本端 1× 与 MAX，至少 3 个独立稳态样本。剔除启动、热身、退出；跨日测试独立，不混入稳态。在 1× 时间窗不足时，报告 `tick/s ÷ 设计 1 tick/s` 的理论基准倍率，不能说是实测 1×。
2. **推进吞吐**：取 authority 完整采样窗的 `sample_ticks / sample_duration_ms`，再用 authority 起止 tick/seq 独立复核。查询低频，避免每拍抢占控制优先。WS 或 UI 的基线 tick 不能代替 authority tick。
3. **连续送达吞吐**：计算实际增量帧连续 tick / seq、字节数、缺口与 baseline 重连次数。重连前后分段，不计被跳过的帧。独立记录收到每帧时间，没有对齐时不能声称多少是编码或网络开销。
4. **端到端 UI**：相同视口观察 `[data-game-tick]`、图表数据和绘制间隔。可单独测控制 ACK 到真实唯一订单事件可见，不把该值当作所有行情包单向延迟。
5. 同时报本场景 `MAX / 1×` 和同场景 `candidate / baseline`；仅在测量层、种子、资源与工作量一致时横向比较。正式基线与候选按实际源码/产物 SHA、CPU affinity 与窗口记录。同一产品的前后回归门槛须先定；不能用不同层的现成均值推算三端倍数。

现有工具：`scripts/libos-product-parity.mjs` 是单步 authority 见证，不是 MAX；`scripts/stalled-observer-day.mjs` 是 Browser 停 ACK 的 Vite fixture，不是 UI 性能样本；`scripts/performance/market-ui-report.mjs` 走真实 Chromium CDP，目前默认本地路径；`production_actor_performance` 是无窗口 Desktop actor 诊断。
