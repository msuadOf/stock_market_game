# 架构 (Architecture)

本文描述当前项目的分层、宿主和数据边界。具体契约以代码及对应 ADR 为准；交易规则的实现范围和简化见 [trading-rules.md](trading-rules.md)，完成情况见 [work-status.md](work-status.md)。工程约束见 [principles.md](principles.md)。

## 分层与依赖方向

```text
React UI / Redux
       ↓ EngineHost
浏览器 Worker  |  远程 server  |  Tauri actor
       ↓                  ↓               ↓
   WASM 绑定        Rust engine      Rust engine
       └──────────────────┴───────────────┘
```

`packages/engine` 负责规则、市场模拟、撮合、账户和组合计算，不依赖 UI、网络或持久化。宿主负责计时、传输和存储，把用户动作交给 engine，并向 UI 发布结果。核心状态与规则可独立于渲染层测试；同一 engine 可由浏览器、server、desktop 和测试分别运行。引擎内部的 runtime 使用线程和消息通道，并不意味着它负责外部 I/O。语言与部署决策见 [ADR-0002](decisions/0002-engine-rust-wasm.md)、[ADR-0005](decisions/0005-unified-engine-three-deployments.md)。

依赖由外向内：UI 通过应用接口访问宿主；宿主和绑定依赖 engine；engine 不依赖任何 app。各 app 不互相导入，desktop 复用 web 构建产物。存储与网络属于宿主边界，不能由 UI 散落地直接访问。

## 宿主与通信

| 宿主 | engine 位置 | 传输 | 持久化 |
|------|-------------|------|--------|
| 浏览器本地局 | Web Worker 中的 Rust/WASM | `postMessage` | IndexedDB 日终快速槽及 JSON 导入/导出 |
| 远程局 | server 的 Rust actor | REST 和 WebSocket | server 的 SQLite 数据库 |
| 桌面本地局 | Tauri 的 Rust actor | Tauri IPC | 前端快速槽及 JSON 导入/导出 |

Web UI 和 desktop 可在启动时选择本地或远程宿主；静态 WebUI 服务不运行浏览器本地局。部署入口和构建目标见 [ADR-0027](decisions/0027-runtime-deployment-and-build-targets.md)，原生数据库与浏览器存储方向见 [ADR-0033](decisions/0033-day-end-database-and-browser-storage.md)。

React 应用通过 `EngineHost` 提交命令，消费统一的 `HostUpdate`：初始化、读档和显式刷新交付 `baseline`，后续 `delta` 声明连续的 seq 覆盖区间。`baseline` 包含权威 `civilDate`；缺失或无效时显式报错。宿主差异通过 `HostCapabilities` 暴露。命令入队确认不等于委托已被接受或成交；详细协议见 [ADR-0010](decisions/0010-unified-host-protocol-and-local-refresh.md)。

## 运行时与状态

native 默认使用 [ADR-0043](decisions/0043-std-thread-ownership-runtime.md) 的所有权转移 runtime：NPC 分组和各股票交易部分别处理决策与撮合，协调者收敛并提交 tick。native 的 `ENGINE_STEP_RUNTIME=0` 是临时旧管线回退口。Web 产品宿主显式启用同一 runtime，经 thread facade 使用 Workers；WASM engine 未设置 override 时仍有串行管线。ADR-0043 仍为 draft，路径存在不代表完整验收已通过。

native 宿主连续运行通过 `ProtocolSession::advance_core_tick` 提交核心，交出拥有同版事实与读面输入的 `DeferredTick`。
普通连续运行在可交换槽中累计 `DeferredInterval`；消费者取走区间后，在锁外只构造并校验末版外围 view，
再由 `record_consumed_interval` 按源 tick 与 seq 顺序登记。后续 tick 可继续在新槽累计，不设固定 8 tick 等待窗口。
显式读取、自然日日界等必要消费点先追平；控制命令响应和区间交付顺序保持。
显式 `step_frame` 仍同步返回完整单步结果，并共用同一核心交易实现。

当前状态保留区间末版；可覆盖的连续报价从完整事实提取为有 seq 覆盖的最小价格观察及同股分钟槽末点，
条件单逐点消费价格观察，图表消费分钟采样，成交、委托反馈、错误、披露和竞价事实保留原身份与顺序。
覆盖规则与接收端验证边界见 [ADR-0010](decisions/0010-unified-host-protocol-and-local-refresh.md)。
`DayEnd` 普通连续拍仍跳过完整 PlanBook 与镜像调度采集，完整 runtime 在批末、显式 flush 或日界按需构建。
`simulation-diagnostics` 使用逐拍同步的外围消费与登记，保留每拍完整投影及诊断；不使用普通累计覆盖，也不建立无界 Full 队列。

核心失败阻止继续推进；外围失败显式报告并停止该交付链，已经提交的 tick 不回退重跑。
未消费事实不能跨 checkpoint 或自然日日结，避免恢复或保存重复交付。

Web WASM 宿主的保守内存总账上限为 3 GiB，每个产品 `WebAssembly.Memory` 的 maximum 为 1 GiB、initial 为 64 MiB，残留重载阈值为 512 MiB。这些约束彼此独立，不代表真实 World 步进或 Browser 二十次验收已完成。

Web WASM 的 panic 早期死亡通知与有界 tombstone 采用受控 std 构建，诊断记录可能缺席或尚未完成；hard trap 由宿主报告。固定工具链、消息限制、构建平台与尚待验证的边界见 [ADR-0044](decisions/0044-wasm-panic-tombstone.md)。该 ADR 仍为 draft，不能将局部探针视为完整产品验收。

正式 WASM facade 初始化会建立 LibOS 故障域并导出共享内存布局；启用 runtime 时，宿主必须登记该布局并在故障时向登记槽广播。缺少正式布局导出的旧 WASM 产物无法启用 runtime 或通过相应恢复能力预检，须重新构建；存档格式本身没有因此改变。默认 Worker 通道仍使用旧等待路径，因此正式导出与握手不代表 LibOS 等待已成为默认路径，也不代表故障后的在途资源已安全回收。

协调者上行接收已通过 LibOS 共同入口：Native 使用 std 的阻塞接收，默认 WASM 使用登记到故障域的通知通道。Worker 下行等待及故障后的在途资源移交仍在迁移，当前不能称整个 runtime 已统一至 LibOS。

混合日历 raw runtime 接口仍在独立审查与组合验证中：`World::spawn(spec, calendar)` 显式接收 `TradingCalendar`；`calendar_start_date` 与当前 `civil_start_date` 是不同事实。`run_ticks(n)` 在执行前拒绝跨当前自然日的区间，raw caller 显式完成日终并设置下一自然日；生产 `GameSession` 仍拥有自然日经营与提交权威。

每个 tick 的失败须显式报告，不能发布部分结果；自然日更新是独立事务。并发受理顺序和重放边界见 [ADR-0017](decisions/0017-escrow-parallel-tick.md) 与 [testing.md](testing.md)。不以固定 seed 承诺不同并发到达轨迹产生相同成交结果。

`SaveSlot` 保存恢复所需的权威事实，仅成功自然日日结产生持久存档候选；恢复时重建 runtime。输入在宿主边界及 engine 中校验，不兼容或损坏的存档显式报错。`Money` 在 engine 内以分为单位，跨 JSON、WASM 和宿主边界使用十进制分字符串；Web 用 `BigInt` 计算，但不将其存入 Redux。具体契约见 [ADR-0025](decisions/0025-day-end-only-persistence.md)、[ADR-0029](decisions/0029-responsibility-names-and-contract-versions.md) 和 [ADR-0031](decisions/0031-money-decimal-cents-wire.md)。

## 目录和 workspace

```text
apps/
  web/                 React、Redux、宿主适配器和浏览器存储
  web-wasm/            engine 的 wasm-bindgen 绑定
  server/              可选远程服务及 engine actor
  desktop/src-tauri/   Tauri 宿主及 engine actor
packages/
  engine/              游戏核心与 runtime
  engine-gpu/          GPU 探测和实验管线
  native-store/        server 使用的原生 SQLite 存储
  std-thread-proto/    runtime 选型原型
```

Rust 包由 Cargo workspace 管理，`apps/web` 由 pnpm workspace 管理。`engine-gpu` 是实验能力，权威计算仍由 CPU 承担。

## 数据流示例

```text
用户提交买入意图
  → React 调用 EngineHost.submitIntent
  → 当前宿主接收命令并交给 engine
  → engine 在市场步进中处理委托和撮合
  → 宿主发布 HostUpdate
  → Redux 更新视图，React 渲染结果
```

边界校验或执行失败时，错误必须携带上下文并由 UI 显示；不能用默认值掩盖错误。错误处理约定见 [error-handling.md](error-handling.md)。
