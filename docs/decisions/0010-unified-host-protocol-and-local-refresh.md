# ADR-0010：统一宿主应用协议与前端局部刷新

- **状态：** accepted
- **日期：** 2026-09-09
- **决策者：** msuad
- **关联：** ADR-0005、ADR-0007、ADR-0009、`docs/architecture.md`、`UX-CONTRACT.md`

## 上下文

同一份 Rust engine 运行在浏览器 WASM Worker、远程 server actor 和 Tauri 原生进程中。
三种宿主的传输技术必然不同：Worker 使用 `postMessage`，server 使用 WebSocket/REST，
Tauri 使用 invoke/event IPC。React 不应知道这些差异，也不能因为换宿主而改变状态更新、错误、
命令确认或分时采样语义。

此前 `EngineHost` 已屏蔽一部分调用差异，但更新仍以 `onEvents`、`onSnapshot` 两个回调表达，
`PublisherFrame`、Worker 消息和 Tauri 事件各自维护相近但不完全相同的顺序规则。WASM 默认
30Hz flush 也与已确认的 UI 至少 60Hz 要求冲突。Redux 对纯行情事件已保持未命中股票分支引用
稳定，但 `App` 仍订阅完整 snapshot，组件级局部刷新尚未落地。

## 本次对话决策核对

| 已确认想法 | 文档状态（本 ADR 前） | 本 ADR 的处理 |
|---|---|---|
| server“最快”不设 1ms tick 上限，以 CPU 时间片持续 loop 并主动 yield | ADR-0005 已记录 | 保持 |
| server 暴露实际倍率，UI 能验证设定倍率与最快倍率 | ADR-0005、UX-CONTRACT 已记录 | 保持统一 `SpeedMetrics` |
| 模拟与观测分离，为单模拟多客户端铺路 | ADR-0005 有原则但协议分散 | 固化为统一应用消息层 |
| UI 更新频率至少 60Hz | 远程 16ms 已记录；WASM 仍为 30Hz | 三宿主统一目标 16ms（约62.5Hz） |
| NPC 当前保持既有并行模型，不增加串/并行动态切换 | ADR-0008 已明确避免无收益复杂度 | 不改 NPC 调度 |
| 每客户端独立 Publisher；写请求由该客户端网关进入交易队列 | ADR-0005 已记录 | 保持 `CommandQueued` 仅表示入队 |
| 只保证进程存活期间的状态，不引入 WAL/崩溃恢复 | ADR-0005 已记录 | 不增加持久消息设施 |
| 行情帧允许覆盖，但分时图不能少分钟点/竞价采样点 | ADR-0005、ADR-0009、UX-CONTRACT 已记录 | 统一 delta 覆盖区间与采样测试 |
| push 与 pull 两种 Publisher 模式供用户选择并可比较性能 | ADR-0005、UX-CONTRACT 已记录 | 作为 remote capability，不伪装成所有宿主都有 |
| 前端局部刷新，不能用每帧全局状态替换 | UX-CONTRACT 有目标，组件层未完成 | 本 ADR 定义状态与订阅边界并开始迁移 |
| 抽象通信过程，屏蔽 WASM/server/Tauri 差异 | 尚无独立决策 | 本 ADR 正式采纳 |

核对结论：先前产品决策没有遗失；缺口是统一应用消息模型、WASM 刷新频率和组件级订阅实现。

## 决策

### 1. 统一语义，不统一底层传输

应用层只消费 `HostUpdate`：

```ts
type HostUpdate =
  | { type: "baseline"; snapshot: Snapshot }
  | {
      type: "delta";
      fromSeq: number;
      toSeq: number;
      events: EngineEvent[];
      runtimeSnapshot?: Snapshot;
    };
```

- `baseline` 只用于初始化、读档和显式重同步。
- `delta` 必须声明覆盖的原始 seq 区间；允许行情压缩造成 `events` 内部 seq 空洞，区间本身必须连续。
- 状态变更事件需要匹配 `toSeq` 的权威 runtime snapshot，整帧先校验后交付，禁止部分应用。
- fatal error 使用结构化 `HostFailure { code, message }`；用户文案由应用层统一生成。
- 命令继续通过 `EngineHost`；有副作用的命令必须等待宿主确认，`CommandQueued` 不等于订单接受或成交。

底层适配器保留真实能力：

| 宿主 | 传输 | 宿主专属能力 |
|---|---|---|
| WASM Worker | Structured Clone `postMessage`；WASM 绑定使用 `serde_wasm_bindgen` | `uiFrame` 背压、SharedArrayBuffer/rayon |
| Remote | WebSocket JSON + REST | push/pull、重连、鉴权、每客户端缓冲 |
| Tauri | invoke + event IPC；Rust actor 侧先聚合固定高倍速 tick，再按 16ms 跨 IPC | 原生进程生命周期 |

不得让 WASM 模拟 WebSocket，也不得让远程宿主伪装成共享内存。差异通过
`HostCapabilities` 显式暴露；UI 只在能力存在时显示对应控件。
Tauri 的 event 与 invoke response 属于两条 IPC 回调路径；每次成功读档必须更换 `timeline_id`，
前端在交付新 baseline 前切换时间线并拒绝晚到的旧时间线事件，不能假设两条 IPC 天然全序。

### 2. 统一观测节奏

- 三宿主的 UI 目标交付周期为 16ms（约 62.5Hz），满足至少 60Hz 的产品要求。
- 浏览器实际绘制仍由 `requestAnimationFrame` 决定；后台页、60Hz 面板和繁忙主线程可能降低实测刷新率，
  不得把目标频率冒充为硬实时保证。
- 最快模拟循环不等待 UI；上一帧未消费时只压缩允许覆盖的行情采样。委托、错误、集合竞价结束和日界完整保留；三宿主的可见成交带统一只保留最新 100 条，与 Redux 容量一致，不承诺 UI 内的完整逐笔历史。

### 3. 局部状态与订阅

- 全量 snapshot 只作为基线；纯行情 delta 按股票代码修改 `markets[code]`，不替换账户和其它股票。
- 游戏时钟订阅 `day/tick/phase`；个股详情和分时图只订阅当前代码；账户/持仓只订阅玩家账户及持仓代码；
  成交与委托使用独立 slice。
- `App` 不再订阅高频完整 snapshot；时钟、行情、图表、账户/持仓、成交与移动详情各自在组件边界订阅最小状态。
- 行情容器订阅完整 `markets` 集合以发现动态增删代码，但 AG Grid 使用稳定股票代码作为行 ID，并按代码比较后调用异步行事务；单只股票 tick 只更新命中行，未变化行保留引用和 DOM 行。
- 当前 engine 对成交/委托仍返回完整 runtime snapshot；在账户增量事件建模完成前允许这些低频状态变更换基线，
  文档与 UI 不得声称已经完全消除全量替换。

## 不做

- 不新增 NPC 串行/并行动态切换。
- 不引入统一字节编码、消息中间件、WAL 或跨进程可靠队列。
- 不为追求形式统一删除 remote push/pull 或 Worker `uiFrame` 背压。
- 不在没有剖析数据时把所有 React 页面一次性重写。

## 后果

- React/Redux 可用同一更新入口运行在三个宿主上，协议不变量只维护一次。
- 传输适配器仍可独立优化，不把网络复杂度泄漏到 WASM/Tauri。
- 局部刷新已有可测试的引用、订阅和 AG Grid 行事务边界；成交、委托、集合竞价结束与日界仍诚实保留 runtime snapshot 的低频全量基线。
- 增加一层应用协议类型和适配测试，但减少三处顺序/错误规则漂移的长期成本。
