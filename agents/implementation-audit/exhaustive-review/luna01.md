# Luna01：ADR-0005、ADR-0010、ADR-0027 全文复核

## 阅读范围与复核基线

- 连续从首行读至 EOF：`docs/decisions/0005-unified-engine-three-deployments.md` **144 行**、`0010-unified-host-protocol-and-local-refresh.md` **104 行**、`0027-runtime-deployment-and-build-targets.md` **60 行**，合计 **308 行**。以下按所有章节逐章闭环，不以关键词搜索代替阅读。
- 已读仓库 `AGENTS.md` 与 `docs/principles.md`。源码工作树 HEAD 为 `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`；任务指定产品基线 `08e4fc7`，本报告只依据该工作树当前源码，不写产品文件、不做 Git 写、不运行测试或长任务。
- 对照旧总账候选及 `sweep01.md`、`sweep61.md`、`sweep62.md`、`sweep63.md` 后，以下历史结论均重新按当前 caller → owner → consumer 核验。较新正式 ADR 优先于旧愿景和行为保持记录；ADR-0025/0028 用于判定存档及发布边界。

## ADR-0005 逐章矩阵

| 原文行／章节 | 当前生产 caller → owner → consumer 及复核结论 |
|---|---|
| 1–24 元数据、上下文及约束 | 三宿主主干仍符合统一引擎与可选联机方向；单局/多人同局未实现符合 ADR-0027:58–60 的现行边界。随机状态由 `packages/engine/src/session.rs:129` 等显式持有；普通新局仍从 `apps/web/src/app/useSessionHostLifecycle.ts:97` 使用 `DEFAULT_SEED`，旧 G20 未核销。 |
| 27–45 §1 单引擎／三端同构 | React 启动入口 `useSessionHostLifecycle.ts:98` 创建宿主，Worker、Server actor、Desktop actor 分别推进 engine 协议并交付统一更新；应用层已演进为 session/protocol transaction 与 `HostUpdate`，不是旧字面函数签名缺失。旧 sweep01 认定主干实现仍成立。 |
| 47–51 §2 统一账户 | `packages/engine/src/account.rs` 的 `Account` 是共用账户 owner，玩家意图与 NPC 决策均进入 session 执行/结算链；账户统一不代表所有策略/诊断承诺齐备，旧总账中的具体策略及诊断缺口不由本章核销。 |
| 53–57 §3 共享盘口与撮合价格 | 每股票 session/stock-stream 将已受理请求送入共用订单簿并由账户结算；此处要求真实受理的价时序，不承诺并发中尚未受理请求的到达顺序。总账的具体历史/合成前史候选另依其正式依据处理，不因统一账户模型自动清零。 |
| 59–75 §4 step、随机性、Fastest、测速、更新批次、T+1 | Server `apps/server/src/actor.rs:1083–1094,1120` 在 Fastest 使用有界批次和 yield；Desktop `apps/desktop/src-tauri/src/actor.rs:840–891` 同样按预算批处理；Worker `apps/web/src/host/wasm-tick-loop.ts:92` 对最快路径让出。`readSpeedMetrics` 已有三端接口与读取端，但 Remote `remote-host.ts:197–199` 的 speed metrics GET 缺会话凭据，G02 仍成立。Desktop 普通固定倍率路径 `actor.rs:836–837` 每次 `run_cycle(1)`，之后 `publish_protocol_cycle` 每帧 emit (`:907–920`)，`pending_fixed_events` 在 `:682` 仅声明、`:894–895` 只清理；现有聚合 helper 为 `#[cfg(test)]` (`:1179–1186`)。故 ADR-0010:68 的固定高倍速 IPC 聚合仍缺，G19 仍成立。T+1 外部配置限制及日终候选按现行 engine/ADR-0025，不恢复旧 T+0 产品开关。 |
| 77–83 §5 联机钩子 | Server session registry/handles 负责单局隔离和凭据；这满足接口预留，不代表新增多人同局。旧 G01 浏览器 WS 凭据路径不匹配仍须单列，不能以服务端存在 token owner 核销。 |
| 85–109 §6 双通道及韧性 | Remote REST/WS 入口存在，Server route 有 `GetFrame` 分支 (`apps/server/src/routes.rs:1430–1445`)，但当前 RemoteHost 未运行 pull 的请求帧循环，G03 仍成立。Server heartbeat 只每 30 秒发 Ping (`routes.rs:1389–1394`)，未核对 Pong deadline，旧 G05 仍成立。继续时旧 baseline 重送问题 G04 仍成立。SubmitIntent 的 `CommandQueued` 仅确认队列接收，`routes.rs:1447–1454` 与 ADR-0005:103–104 一致，不把入队当成交。65,536 预算仍按完整协议批次的 units 计量，且 `apps/server/src/publisher.rs:81–90` 允许单一不可分更新本身超预算；对旧 sweep01 的“上限契约债”复核仍有效，但当前证据不足以另称无界内存增长。ADR-0027:29–31 收窄公网 TLS/认证验收，不能要求本轮新做证书或公网网关。 |
| 111–113 §7 持久化 | 三宿主继续使用共同日终候选和各自存储 owner；ADR-0025:12–29、:38–49 是较新契约，限定成功 CivilUpdate 后持久化、日内不写盘、启动只读一次。旧 ADR 的“服务器存档/本地存档”不能推导出自动云同步、WAL 或日内存档。 |
| 115–120 Alternatives | 否决方案不是实现缺口；目前不做完整联机、多人 PvP、随机游走或多份引擎符合决策。 |
| 122–138 Consequences / 后续工作 | account、orderbook、market、session/save 均有当前实现 owner；NPC 具体模型由后续决策细化。种子可重放不等价于不同自由并行调度下整局 byte-for-byte 相同。依赖评估与开放问题的历史行动不据本章新增代码候选。 |
| 140–144 Related | 纯导航与关联规格，不构成额外 runtime caller；各专题以更新后的正式 ADR 为准。 |

## ADR-0010 逐章矩阵

| 原文行／章节 | 当前生产 caller → owner → consumer 及复核结论 |
|---|---|
| 1–18 元数据与上下文 | `EngineHost` 当前以统一 host API 交付更新；`apps/web/src/host/host-update.ts` 与 protocol coordinator 承接统一消息。App 不以旧 `onEvents`/`onSnapshot` 模式消费；16ms 只是目标，不证明真实设备达到 60Hz。 |
| 20–36 决策核对表 | 原表列出的每项按下面决策条款逐项核实。Remote push/pull 仍为 capability，但没有 `GetFrame` 生产循环 (G03)；三宿主同构并未抹除 Worker 背压、Remote transport、Tauri IPC 差异。 |
| 38–60 决策 1：HostUpdate、校验、失败、命令确认 | baseline/protocol 更新经统一 coordinator 校验后交付，fatal failure 有结构化边界。最关键的 ADR-0010:60 命令确认要求尚未全宿主兑现：`apps/web/src/host/engine-host.ts:39–47` 的 `start/stop/setSpeed` 是 `void`；生产启动 caller `useSessionHostLifecycle.ts:136–142` 发 speed、暂停偏好后启动，但 `setSpeed` 未等实际应用。Remote `remote-host.ts:162–166,189–190` 把 running/speed REST 异步丢弃；Worker `worker-host.ts:264–280` 以 `postMessage` 发 start/speed；Desktop `tauri-host.ts:158–180` 对 setSpeed 丢弃 invoke Promise，`apps/desktop/src-tauri/src/actor.rs:558–564` 的方法仅 send mpsc，无 actor 应用回执。Desktop pause-preferences 虽然 TS await invoke，但 actor 的 `:576` 路径也只 send，因此 invoke 成功只确认入队。Server `apps/server/src/actor.rs:815–823,838–848` 对 set_speed/set_running/偏好提供 oneshot reply，是正反宿主对照。控制副作用实际完成前 UI 无法等待确认，旧 sweep01 的 S01-C01 与 sweep63 的 S63-01 是同一候选；复核为 ADR-0010 明确契约缺口候选，需总账去重，不拆成两项。不能把 SubmitIntent 的 `CommandQueued` 扩成成交确认。 |
| 62–74 决策 1 宿主能力与 Tauri timeline | Remote `capabilities.deliveryModes` 声明 push/pull，UI 读取能力并显示；缺 pull 循环仍 G03。Tauri event listener 与 invoke response 分属两通道，前端 timeline 过滤在 `tauri-host.ts`/`tauri-timeline-state.ts`；对读档 timeline 切换有 owner。Worktree/sweep60 记录的 start 再交付缓存 baseline 问题在当前 `remote-host.ts:154–162` 仍能看到，Remote 与 Tauri 均保留旧缓存起始交付路径，正式 baseline 仅初始化/读档/显式重同步 (ADR-0010:56)，G04 继续。 |
| 75–80 决策 2 观测节奏与采样 | Worker 更新交付、Server push clock 与 Tauri IPC owner 均存在；Desktop 固定速度每 tick 走 `run_cycle(1)` 并在每周期 emit (`actor.rs:836–837,907–920`)，没有生产的 16ms 聚合 owner，G19 继续。固定周期常量、Fastest 批次或测试 helper 不能核销 G19。最新 100 条可见成交带不承诺完整逐笔历史。 |
| 82–90 决策 3 局部状态订阅 | App 与 store 有局部订阅/增量更新，AG Grid 以股票代码差分。Sweep01 指出的 chart 引用隔离 G31 仍属于其他既有候选，本章主干完成不能核销 chart 专项。成交/委托低频全量基线仍被 ADR 明确允许，不宣称全状态已无全量替换。 |
| 92–97 不做 | 不做清单排除了动态 NPC 调度切换、统一字节编码、WAL、删除 Remote pull、无数据的大规模 React 重写；不得将其记为本轮漏实现。 |
| 99–104 后果 | 三宿主统一入口、组件边界和能力差异均有真实代码路径；目标频率、性能提升及视觉验收不能由代码常量/历史短测推定。 |

## ADR-0027 逐章矩阵

| 原文行／章节 | 当前生产 caller → owner → consumer 及复核结论 |
|---|---|
| 1–13 用户决定 | 同一生产 UI 启动可选择本地或 Remote，不为模式分别编译；允许独立 WebUI、Server、合体部署。此决定明确不扩大账号、公网适配或网络环境产品范围。 |
| 15–23 决策 1–2：构建目标和启动选择 | `scripts/build-targets.mjs` 声明 desktop/webui/webui-server/server；启动选择通过 startup target 建宿主，本地仍用访问者 WASM Worker，Remote 复用 RemoteHost。模式只在启动阶段选择，无运行中无缝迁移符合原文。测试构建 E2E 路径与生产边界分开。 |
| 24–28 决策 3：静态 WebUI 服务及服务模式 | Rust/Axum `apps/server/src/main.rs` → `deployment.rs` → `web_ui.rs` 提供静态资源；`--services` 分支区分 webui/server/all，纯 Server 拒绝 WebUI 服务。没有用 Vite preview/Node 作为部署 daemon。 |
| 29–31 决策 4：HTTP/WS、代理 TLS、监听地址 | 原生服务使用 HTTP/WS，监听默认回环并允许显式配置；会话凭据校验在私有路由保留。TLS/证书签发与公网安全验收明确不在本轮；不能用此 ADR 核销 G01 客户端浏览器鉴权适配。 |
| 32–35 决策 5：COOP/COEP 与线程错误 | Rust 静态响应提供 COOP/COEP；Web startup 在 Worker 创建前检查安全上下文/SAB 并显式说明解决路径，没有静默单线程、主线程或 Remote fallback。普通 HTTP 跨设备的限制已明确保留。 |
| 36–38 决策 6：构建依赖与隔离 | `scripts/server-build.sh` 直接 Cargo，独立 server 目标不要求 Node/前端资源；其余构建机依赖 Node、pnpm、Rust 属构建端要求，不是部署依赖。 |
| 39–40 决策 7：原生平台 | workflow 为 Windows/Linux/macOS 原生 runner 构建；可在三平台运行不等于当前 Linux 跨编全部安装包。交叉编译实验不构成支持承诺。 |
| 41–43 决策 8：编译与完整回归 | ADR-0028:70–89 是更新的发布/手动入口规则：Release 和产品手动构建只构建/打包/核验，CI 仅独立手动诊断。发布中不跑测试、lint、Clippy、E2E 是明确政策而非漏测。当前审计未运行验证，不引用历史通过充当本轮验证。 |
| 45–56 边界：制品、发布及 crate 类型 | distributions/release workflows 与 `scripts/package-distributions.mjs` 承担各平台打包；ADR-0028 更新了早期 artifacts-only 范围，当前有效标签发布及 Pages 规则以其为准。仅产出 `rlib`、不做移动宿主/签名/公证均符合明确边界；制品代码存在不代表真实安装 GUI 已验收。 |
| 58–60 边界：交易、存档与多人同局 | 单一 engine、A 股交易约束、单位、个人策略和存档格式不因 build target 改变。ADR-0025 日终保存契约优先；本地局互相独立，Remote 客户端不共享玩家账户，未实现多人同局符合决策。 |

## 旧候选重核与新增候选反证

- **保留已有 G01–G05：** G01 浏览器 WS token/query 与服务端 header 鉴权路径不匹配；G02 Remote speed metrics 未带凭据；G03 pull 模式没有生产 `GetFrame` 轮询；G04 恢复/继续时重送可能过期 baseline；G05 heartbeat 没有 Pong deadline/自动恢复边界。逐项证据与 sweep01/61 的旧结论吻合，三篇 ADR 的统一抽象或 capability 声明不能核销。
- **保留已有 G18/G19/G20：** G18 Worker `uiFrame` 背压缺生产落实；G19 Tauri 固定倍速未按 16ms 聚合跨 IPC；G20 普通新局仍固定 `DEFAULT_SEED`。其中 G19 已重查真实 production `tick_and_emit → run_cycle(1) → publish_protocol_cycle → emit_update`；不是只看旧总账。Fastest 14ms CPU batch 是不同路径。
- **重新合并命令确认候选：** sweep01 的 S01-C01 与 sweep63 的 S63-01 指向同一正式 ADR-0010:60 契约，当前跨 Worker/Remote/Desktop 生产命令仍有发出即返回/只确认入队路径。认定一个待总审计纳入总账的候选；Server oneshot reply 是明确反证，不把错误扩大成“所有宿主皆无确认”。
- **sweep62 C62-01（旧 socket onerror 影响新连接）：** 当前 `apps/web/src/host/remote-host.ts:117–134` 为新连接建立 identity，但 `onmessage` 通过 identity 处理、`onclose` 检查 `isCurrentConnection(identity)`，`onerror` 第 129 行不校验 identity，调用全局 `fail` 并可 detach/关闭当前连接。故旧连接迟到 error 仍可能令新连接 fatal，确认其为 G05 生命周期恢复边界的独立具体触发面，不重复新编号。
- **sweep61 C61-1/C61-2：** dispose 没有拒绝所有 pending command、重同步 waiter 单槽覆盖导致旧 Promise 不 settle 的源码事实仍在旧 sweep 记录中；但本任务聚焦三篇 ADR，只有 ADR-0010:60 的副作用确认直接构成正式依据。不能把主动 dispose 清理或并发 refresh 排队策略伪称 ADR 明确要求；作为具体请求生命周期候选留总账/原审计裁定，不能用本次 OOP 提取记录批准静默悬挂。
- **旧公网说明、buffer 上界、旧消息类型名称：** ADR-0005:109 代理配置/CDN 描述被 ADR-0027:12–13、:29–31 收窄，不升级成当前公网产品范围。`PublisherFrame`/HostUpdate 名称和原始事件预算已演化为 `EngineUpdate`/tick/civil 协议；不可分单批超限例外在 publisher 有注释及显式实现 (`apps/server/src/publisher.rs:81–90`)，是契约精确度/背压上界债，尚无证据证明无界积累，不另立运行时 G。
- **其他反证与边界：** ADR-0028 的 build-only 发布规则反证“发布漏跑测试”；ADR-0025 反证“需日内写档/启动重复读档”；ADR-0027 的 TLS、签名、公网适配和跨平台原生构建边界反证把未来承诺扩成当前实现缺口。本轮没有发现另一个可独立确认、此前未登记的产品候选。

本报告只提交全文审计证据与候选给总审计归并；不宣称运行测试、构建或发布验收通过。
