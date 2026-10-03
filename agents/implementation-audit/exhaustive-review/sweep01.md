# sweep01：三宿主、统一协议与构建目标全文复核

## 阅读与基线

- 已连续全文阅读 ADR-0005 第 1–144 行、ADR-0010 第 1–104 行、ADR-0027 第 1–60 行，合计 308 行，均读至 EOF；没有以关键词命中替代全文。
- 已读根 AGENTS.md、docs/principles.md、现有总账与 reaudit-host.md；补读现行 ADR-0025、ADR-0028 全文、open-questions 第 1–90 行及相关生产代码。工作文件路径未发现更深 AGENTS.md。
- 源码基线为 b76ece3。工作树 HEAD 在审计期间为 4ad5a2e；`git diff --stat b76ece3 -- apps packages scripts .github` 无输出，故以下生产证据仍绑定 b76ece3。未修改产品、未运行测试/构建、未进行 Git 写操作。
- 判定采用最新决定：多人同局、公网认证产品、WAL、真实跨平台 GUI 验收不能从旧部署愿景强行扩为当前实现；发布链路依 ADR-0028 仅构建；持久保存依 ADR-0025 日终候选；同 seed 不保证不同自由调度产生相同受理轨迹。

## ADR-0005 逐章闭环

| 全文章节及原文位置 | 判定 | caller → owner → consumer 的证据与边界 |
|---|---|---|
| 元数据/上下文 1–24：「一份 engine」「不在本阶段实现联机」 | 主干已实现；联机未来 | `apps/web/src/app/useSessionHostLifecycle.ts:200` 按 target 选择 Worker/Remote/Tauri；三宿主均拥有 Rust GameSession 并由统一协议 reducer 消费；账户系统/PvP 未开工符合第 18 行。外部输入与存档严格校验由 setup/存档/协议边界承担。 |
| §1 27–45：三端同构，宿主无关应用层 | 主干已实现；具体旧接口文漂 | Worker `wasm-tick-loop.ts:79`、Server `actor.rs:1120`、Desktop `actor.rs:840` 调统一 engine 协议 step，交付 `HostUpdate`，由 `protocol-coordinator.ts:56` 消费。字面 `step(session)->(session',events)` 已演变为有 owner 的 GameSession/协议事务，不因签名不同登记功能缺口。 |
| §2 47–51：「Account 是唯一参与者」 | 已实现 | `packages/engine/src/account.rs:95` 为统一 Account，`:194` 同一构造；`session.rs:2518` 玩家请求与 NPC 管线最终进统一股票受理/结算。个体策略区别不改变同账户撮合；G07 分析能力装配另有缺口，不重复计算为账户模型失败。 |
| §3 53–57：共享盘口、撮合价格、价时优先 | 主干已实现；简化按现行规则登记 | Session 每股票 owner 及 `session/pipeline/stock_stream.rs:285` 就绪受理进入同一订单簿；账户结算 `account.rs:494` 共用。并发实际受理决定时间优先，不承诺按未受理请求生成顺序成交；不从旧「价格内生」要求删除获批合成前史。 |
| §4 59–75：种子、宿主循环、Fastest、测速、批次、T+1 | G02/G18/G19/G20 仍在；其余主干已有 | PRNG `session.rs:129`、Session `:1137` 显式保存；生产新局 `useSessionHostLifecycle.ts:90` 仍取固定 DEFAULT_SEED。Server `actor.rs:1086` semaphore、`:1094` yield、`:1117` CPU 批次，Desktop `actor.rs:859` 14ms、Worker `wasm-tick-loop.ts:92` 每 step yield 已保留控制调度。固定 Desktop 每 tick emit 仍缺聚合。`EngineHost.readSpeedMetrics`→UI polling 已接，但 Remote GET 无凭据。`SessionSetup` 在 `session.rs:997` 拒绝 T+0，`Account.unlock_t1_positions` 在 `account.rs:227`，不把纯函数接缝当产品 T+0。 |
| §5 77–83：多账户设计钩子、WS token | 隔离/ID已有；G01仍在；联机未来 | `apps/server/src/actor.rs:890` 注册表按 session 保存 handles，`:946`/`:958` 创建 session ID/token。类型和鉴权 owner 存在；浏览器 query token 与 header-only handler 不匹配属于 G01。登录/同局多玩家不在当前范围。 |
| §6 85–109：双通道、push/pull、缓冲、确认、心跳、TLS | G01/G03/G05仍在；确认新增候选 S01-C01；部分文漂/证据债 | REST 与 WS 生产入口 `remote-host.ts:34`/`:120`；SubmitIntent 经 `routes.rs:1447`→actor enqueue 后返回 CommandQueued，消费者 `remote-host.ts:100` 只解入队 Promise。pull handler `routes.rs:1430` 已有但适配器无 GetFrame。heartbeat `routes.rs:1389` 无 Pong deadline。TLS 外置按 ADR-0027:29 核销内置证书要求；代理文档 `docs/build-and-deployment.md:260` 有 Upgrade/隔离头约束，但没有 Nginx/Caddy 具体配置和 CDN 限制说明，属旧公网部署文漂，不扩当前产品。帧结构/缓冲差异见下文。 |
| §7 111–113：序列化校验与宿主存储 | 现行日终契约已有；原三端存储承诺被细化 | engine 日终候选→三宿主 save→`apps/web/src/save/day-end-persistence.ts` 写快速槽/已选文件；ADR-0025:18 明确同一候选和禁止日内写。旧「服务器存档/Tauri本地文件」不能解释为当前必须自动云存储，open-questions.md:67 将数据库/云同步列未来。Server `actor.rs:1352`/`:1354` 取候选，不要求 WAL。 |
| Alternatives 115–120 | 否决项，不计漏实现 | 未采用串行三套实现/随机游走/当前完整联机符合决定；不要求恢复。 |
| Consequences 122–138 | 模块主干完成；重复承诺对应上述缺口 | account/orderbook/session/save/NPC 模块已装配；open-questions.md:73/84 明确 Q8/Q9 已决，NPC模型由后续 ADR 细化。原「同种子可重放」由实际受理事实范围约束，不重新提出自由调度整局字节一致。 |
| Related 140–144 | 导航，无独立实现任务 | 所链 ADR/规格的专题由其他 sweep 覆盖，本章没有额外 runtime caller。 |

## ADR-0010 逐章闭环

| 全文章节及原文位置 | 判定 | 生产证据 |
|---|---|---|
| 元数据/上下文 1–18 | 背景中的 onEvents/onSnapshot/30Hz 已替代；局部刷新主干完成 | 当前 HostUpdate 两种更新 `host-update.ts:11`，默认 `UI_UPDATE_INTERVAL_MS=16` 在 `:3`；App 订阅 `App.tsx:134` 至 `:142` 不再订阅完整高频 snapshot。 |
| 对话决策核对 20–36 | 逐行覆盖；G02/G03/G18/G19 等未完成 | CPU片/SpeedMetrics/隔离/队列无 WAL 已有；remote push/pull 的 UI 控件已存在但 pull 仍未连；16ms 是目标而非硬实时；行情采样完整性不得以能力声明证明。未改 NPC 动态调度符合第 28 行。 |
| 决策1 40–60：统一更新、原子校验、fatal、有副作用命令确认 | 更新校验已有；G04仍在；新增 S01-C01 | `ProtocolCoordinator.applyProtocol` 在 `protocol-coordinator.ts:97` 完成 reducer 再发布，异常生成结构化 failure；`:119` 保留低频全量刷新。旧 delta 的具体字段已变成 generation+TickBatch/CivilUpdate，不能只因类型名不同判漏。baseline 重送违约见 G04。第 60 行的控制命令确认尚未兑现，详见新候选。 |
| 决策1 62–74：宿主能力和 Tauri timeline | G18/G19仍在；能力与 timeline 主干已有 | Worker capability `worker-host.ts:247`、Remote `remote-host.ts:153`、Tauri `tauri-host.ts:150`；UI 按 `deliveryModes` 显示控件。Tauri listener `tauri-host.ts:116` 拒绝旧 timeline，读档安装新 timeline `:228`，没有将 event/invoke 当全序。Remote reconnect:true 与实现不符已归 G05。 |
| 决策2 75–80：16ms、绘制、背压、交易事实保留、100条 | G18/G19仍在；未实测目标节奏 | Worker 每步 `wasm-tick-loop.ts:68` post；Desktop 固定每步 `actor.rs:837`/`:915` emit；Server `routes.rs:1320` 16ms push tick。引擎/协议提交帧必须保留，不得为了 UI 频率丢弃交易事实。最新100条是可见成交带，不是永久逐笔历史。运行频率/后台页需独立验收，不从 16 常量宣称已达 60Hz。 |
| 决策3 82–90：局部 state/组件/AG Grid | 主干已有；G31针对 chart accessor仍在 | `App.tsx:134` 最小订阅；Store `store.ts:77` applyRuntimeDelta 保留未改数据。`MarketGrid.tsx:41` 枚举动态代码，`:59` code 行ID；`market-grid-rows.ts:32` 未变 source保留行引用，`market-grid-row-synchronizer.ts:40` 差分→`:42` applyTransactionAsync。不能把 chart 每批数组重建 G31核销，也不能把「低频允许全量」误写成完全无全量。 |
| 不做 92–97 | 明确排除，非缺口 | NPC 串并动态切换/统一字节编码/WAL/删 remote pull/无依据重写均不启动。 |
| 后果 99–104 | 结构主干有；能力兑现见上述缺口 | 三宿主协议 consumer统一、底层各自保留；完整性不等于生产吞吐或视觉验收通过。 |

## ADR-0027 逐章闭环

| 全文章节及原文位置 | 判定 | caller → owner → consumer 的证据 |
|---|---|---|
| 元数据/用户决定 1–13 | 主干已有；远程可玩受G01/G02/G03阻断 | 同一 App 启动选择，生产无需按 local/remote分编译；四 target已存在。用户决定「不扩大公网」是旧公网承诺判文漂的依据。 |
| 决策1 17–19 | 已实现 | `build-targets.mjs:13` 四 target；`:86` Desktop原生，`:101` server/WebUI可执行；WebUI静态服务 `web_ui.rs:97`，本地引擎仍访问者 Worker。 |
| 决策2 20–23 | 已实现启动链；普通新局G20不因此核销 | `App.tsx:700` 生产 target null、`:726` 选择屏→`:733` resolve；`startup-policy.ts:17` 仅 e2e跳过；`useSessionHostLifecycle.ts:82` 选择后一次 read→`:92` createHost→`:100` load。无缝迁移未实现符合范围，失败返回选宿主不重读已消费快速槽。 |
| 决策3 24–28 | 已实现 | `main.rs:15` parse→`:25` executable相对root→`:32` router；`deployment.rs:127` 拒绝未编web-ui的静态服务模式，`:175` 分 server/webui/all；`web_ui.rs:132` 静态服务不开放api/ws。 |
| 决策4 29–31 | 已实现原生HTTP/WS和回环；TLS产品排除 | `deployment.rs:118` 默认127.0.0.1:3000，`:126` 校验；`main.rs:33` 监听；`routes.rs:1068` 保留凭据校验，不能为G01开放私有路由。公网安全验收与证书签发当前没有承诺。 |
| 决策5 32–35 | 已实现；真实环境验收单列 | `web_ui.rs:106` COOP/COEP；`startup-policy.ts:30` 明确缺失安全上下文/SAB原因和解决方法→`:73` 本地检查，`useSessionHostLifecycle.ts:83` 创建前复查。无静默单线程/远程 fallback。 |
| 决策6 36–38 | 已实现 | `scripts/server-build.sh:78` 直接Cargo，无frontend/Node；bat→server-build.ps1。通用 `build-targets.mjs:79` 纯server不准备前端；`:77` jobs/cargo目录按target隔离，`:102`纯server制品无webui。 |
| 决策7 39–40 | 已实现原生矩阵；交叉/GUI未来 | `.github/workflows/distributions.yml` native/server三OS job和runner原生target；不要求本Linux生成全部安装包。 |
| 决策8 41–43 | 已按最新build-only；不是测试遗漏 | ADR-0028:15–18明令发布仅构建，`distributions.yml` 为共享frontend+原生构建/打包；构建 `build-targets.mjs:77` 明确jobs；短测/定向编译/独立审查是实施验证，此次仅审计未重跑。 |
| 边界47–56 | 目标/分发代码已有；发布由0028替代初始artifacts-only | 同轮 shared frontend→native下载，server job不 needs frontend且不安装Node用于编译；制品打包 `scripts/package-distributions.mjs` 覆盖三平台要求。Node用于后续归档/manifest不等于纯Server编译依赖Node。不签名、公证、移动宿主不列遗漏。桌面Cargo库保持rlib。 |
| 边界58–60 | 无新增交易任务 | 一份engine、单位/日终档/个体状态保留；local各局独立、remote新会话不自动共享玩家。无多人同局符合决定。 |

## 新候选、文漂与证据债

### S01-C01：控制命令未等待宿主确认

原文 ADR-0010:60：「有副作用的命令必须等待宿主确认」。当前 `EngineHost` 把 `start/stop/setSpeed` 定义为 void（`apps/web/src/host/engine-host.ts:39`、`:44`、`:46`）。生产 caller `App.tsx:364` 调 stop 后立即 `setRunning(false)`，`:369` 调 start 后立即 `setRunning(true)` 并在 `:374` 显示「已继续模拟」；初始化 `useSessionHostLifecycle.ts:139` start 后 `:175` 同样宣告 running。没有等待权威处理完成。

owner方面：Remote `remote-host.ts:162`/`:166` 的 running REST 被 void丢弃，`:190` 调速同样fire-and-forget；Tauri `tauri-host.ts:158` 内部虽有invoke Promise且确认后才设私有running，无法让外部caller等待，`:163`暂停与`:180`调速也如此；Worker `worker-host.ts:264`/`:279` 和 `worker-lifecycle.ts:21` 只是postMessage，`wasm-worker.ts:170` 收到后才实际处理，无确认回执。失败最终fatal显式展示已有，不可写成静默吞错，但“先宣称已执行、再异步失败”违背确认契约。

该候选独立于G04：G04是继续时重送旧baseline导致回退，本项是控制状态的确认时序；即使修掉baseline重送仍会发生。也独立于G02的测速认证。建议由总账复核后单列现行缺口，最小测试为延迟/拒绝pause/resume控制命令时UI不得提前宣称完成，重复点击不产生相反控制结果；不要求WAL或保证进程退出可靠投递。调速UI本可表达“请求值”，优先确认pause/resume的明确成功文案，不把所有请求值展示都误判为成交确认。

### 文档与实现需对齐，但暂不冒充新增G

1. ADR-0005:95–102仍描述压缩 `PublisherFrame {from_seq,to_seq,events,runtime_snapshot?}`，ADR-0010:45–53仍为delta。当前统一协议是 `HostUpdate {type:protocol, generation, update}`（`host-update.ts:20`）及TickBatch/CivilUpdate（Server `publisher.rs:55`）；`docs/architecture.md:33`也仍称baseline/delta。统一消费者、连续cursor/原子校验存在，不因旧字段不存在判删除协议能力。若后续文档批准证据不足，应补契约修订记录。
2. 当前 Publisher 保留完整协议批次而不覆写TickFrame（`publisher.rs:93`），65536预算在非空backlog时生效，空队列允许单个合法大更新超限（`:82`–`:85`）。原文「原始事件预算」与不可分批次例外不一致。这属于准确界定背压单位/上界的契约债：不能用删提交事实或任意业务请求配额修复；未验证它导致无界积累，不直接新增内存泄漏G。
3. ADR-0005:109的代理具体配置/CDN说明缺失，但ADR-0027:13/29–31收窄公网环境适配；只登记旧公网文漂，不能据此要求本轮建设证书/网关。
4. 16ms、UI局部引用、三平台制品代码能证明实现路径，不能证明所有硬件60Hz、性能收益、GUI安装与当前线上状态；本次未跑任何测试，不引用历史绿灯作本次验收。

## 总账变化建议

- G01、G02、G03、G04、G05、G18、G19、G20 均仍成立，无核销；G19只限固定倍率聚合，Fastest批次已有；G20只限生产新局取种，不否认RNG存储/恢复。
- G31局部chart引用缺口仍存在，AG Grid行引用/事务已实现，不把二者合并核销。
- 新候选1项：S01-C01控制pause/resume确认时序，待独立复核后进入总账。该三篇范围未改变原Q项分类，未发现能凭本次证据核销的旧Q。
- 文漂3类：旧HostUpdate/PublisherFrame形状及压缩措辞、Publisher单帧超预算例外、旧公网代理说明；验收债：节奏/性能/GUI/线上环境。未扩未来产品。
