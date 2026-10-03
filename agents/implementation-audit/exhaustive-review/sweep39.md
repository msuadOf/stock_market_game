# Sweep39：Web 使用说明、架构与 CI 修复全文复核

## 范围与基线

- 全文从首行连续读到 EOF：`apps/web/README.md` **57 行**、`docs/architecture.md` **171 行**、`docs/ci-build-fixes.md` **23 行**，共 **251 行**。
- 使用目标源码 `b76ece3`；worktree HEAD 为后续文档提交，已比较本批三文档及相关Worker、构建、Web test生产脚本，目标至HEAD无相关产品差异。
- 复用已读根AGENTS/principles/open-questions，并对照ADR-0027/0028；发布与产品手动入口遵循build-only，维护CI保留独立测试。没有运行产品、构建、测试、联网或Git写操作；本文件不修改产品。
- 本组关注运行形态、Worker、安全上下文、部署与正式工具消费；架构下的交易内核逐条仅确认真实生产入口，不把存在函数当成全部领域行为已经正确。

## 全文逐章条款台账

| 原文章节/条款 | 当前生产调用与实际消费 | 状态、反证与边界 |
|---|---|---|
| Web README 1–7：共享React连接wasm/remote/tauri | `useSessionHostLifecycle.ts:204` 按StartupTarget分别调用三createHost；`engine-host.ts:38` 共用EngineHost接口 | 主干已有；UI不必为了形式统一把Worker变WS。远程实际连接问题仍G01–05，不由三分支存在核销 |
| 本地运行 9–17：两平台WASM脚本构建/复制/安装/Web构建 | `wasm-build.sh:5`、`.bat:4`委托frontend-build；`frontend-build-plan.mjs:15` frozen install、`:16` wasm-pack、`:20` copy-wasm、`:22` tsc、`:23` vite；`frontend-build.mjs:69`实际复制 | 已接正式工具；`:90` 外部300000ms监督及1000ms清理预留；jobs传Cargo/Rayon。未执行首次冷构建 |
| 本地运行 19–21：同制品启动选择、自动本地宿主、远程地址及DEV初值 | `App.tsx:700`启动target、`:701` DEV默认；`:726` StartupScreen；`startup-policy.ts:53` local区分desktop/wasm，`:36`验证HTTP(S)地址；`remote-request.ts:10`派生WS/WSS | 主干已有；E2E明确特例`initialStartupTarget:15`不算生产绕过启动选择；没有无缝运行中迁移承诺 |
| 同章 22–23：DEV token与构建变量剔除 | `remote-host.ts` DEV token条件；`build-targets.mjs:321` compilerEnvironment删除三个VITE变量、`:330`真实spawn消费；`:308` staged frontend排除.env文件 | 已接构建生产路径；不因README曾用环境变量就要求每模式另编译。未审查实际发行bundle内容 |
| 同章 24：失败/取消切换等待已提交存档、复用启动读档与配置、无降级 | `App.tsx:706` returnToStartup停当前会话、`:714` invalidate、`:715` idle后改target；`useSessionHostLifecycle.ts:90` InitialSaveSource只读一次、`:96`取档setup、`:105`load；`:181`失败释放宿主并显错 | 主要生产链存在；存档原子性与迟到I/O由持久化分组复核，不据单个idle调用宣称所有边界通过；G20固定新局seed另记 |
| 同章 25：浏览器本地多线程安全上下文COOP/COEP | `startup-policy.ts:26`同时检查secure/isolated/SAB并给原因；`:57` 创建前检查；生命周期`:89`再次检查；Worker初始化`:129` initThreadPool | 已接主调用，明确失败不自动单线程/主线程/远程fallback；远程启动不要求浏览器本地WASM能力 |
| 同章 27–28：生产Rust服务，无Node/Vite preview部署依赖 | `server/deployment.rs:176`选择静态/API/合并router；`:157`可执行位置默认webui；`web_ui.rs:97`static_router；Server manifest可选web-ui；纯Server脚本直接Cargo | 已实现部署主干；仅WebUI静态模式不创建远程engine会话；普通HTTP非回环多线程受浏览器限制不是漏TLS实现 |
| DEV诊断 30–41：独立WASM调试包、仅DEV精确开关、同局seed/setup | `wasm-worker.ts:111` DEV条件、`:112`精确flag检查、`:116`动态导入diagnostic包；普通/生产`:119`/`:123`正式包；`:142` slot.create消费同一setup/seed；`:145`能力协商 | 真实入口已有；`wasm-build-mode.ts:1`要求dev&&flag===1；`.cargo/config.toml:16` build-std补足README命令未显式-Z的配置。反证“诊断包只有文档、无加载路径”不成立；G37真实订单ID追踪不由诊断包接线核销 |
| 验证 43–53：web test/lint/build/e2e、生产preview与绑定前置 | `apps/web/package.json:8`生产tsc/vite及release-WASM检查、`:10`正式web test runner、`:11`Playwright；`playwright.config.ts:19`e2e构建production preview，`:8`fullyParallel、`:9`2workers | 工具入口已有；README说E2E使用preview不等于生产部署使用preview。G26裸oxlint warning门槛仍另记；E2E未在本批执行 |
| 验证 55–57：Rust权威规则、前端仅及时反馈 | `useTradingCommands`组装intent→EngineHost；Worker enqueue→session；交易规则/架构引用 | 规则边界主干存在；不能由前端校验存在推导实现全部A股制度，也不能将已登记真实制度简化重新列成隐藏必做 |
| architecture引言 1–6及§1 10–35：核心纯逻辑、同引擎三宿主、统一应用协议/能力 | 三Rust应用manifest依赖engine；engine manifest无Web/网络/外壳依赖；`EngineHost:38`共用接口，`HostUpdate:11`当前baseline/protocol消息 | 分层主干已实现；原文baseline/delta名字过时见D01。G01–05、18、19各有实际行为缺口，不以接口统一核销 |
| §2 37–55：四层与无跨外壳依赖 | Rust manifests `apps/web-wasm/Cargo.toml:19`、`apps/server/Cargo.toml:26`、`apps/desktop/src-tauri/Cargo.toml:31`各下依赖engine；UI生命周期集中I/O装配，交易内核不import React/DOM | 主干存在。Web导入WASM绑定是文档明确的适配层消费，不用笼统“apps之间不得依赖”否定既有绑定路线 |
| §3 57–75：可替换本地/服务端/桌面存储，四目标三服务模式 | deployment CLI `:82`三枚举、`:130`无web-ui拒绝static、`:181`纯server仅API、`:189`static-only、`:190`all；`build-targets`真实四target构建；运行选择如上 | 已有现行目标与服务能力。数据库/多人权威后台是阶段愿景，ADR0027明确不新增多人同局，见D02，不判隐藏漏实现 |
| §4 77–83：可序列化纯事实、单一存储入口、schema显错 | `App.tsx:114`唯一浏览器repository装配；`useSaveCommands.ts:63`显式加载、`:70`validateDayEndArchive、`:71`失效旧写、`:73`idle；engine恢复深度校验 | 主干已有；日终保存优先ADR0025。不能要求日内再次存完整交易队列来恢复旧文档泛化持久化；没有宣称全层边界已测 |
| §5 85–106：monorepo目录、engine叶子、GPU实验管线 | Cargo workspace/前端pnpm workspace、三应用manifest、engine-gpu目录都有；核心没有app依赖 | 已有目录及依赖方向；GPU能力探测不承诺现行生产GPU后端切换，Q09仍为旧ADR范围歧义 |
| Escrow边界 108–119：同一生产P0–P9、预算1不另引擎、资源/单位/公开私有隔离 | `pipeline/authoritative_tick.rs:20`统一dispatcher，各TradingPhase分别prepare→commit；`decision_resources.rs:84`真P1seal；快照公开玩家信息，内部验证读取完整session事实 | 主干已有；G07–09/G16/G28/G35–38等链路细项不由架构图核销；不会将线程预算1当第二引擎 |
| 同章 121–131：fatal业务区分、原子P9、独立CivilUpdate、真实提交证据 | authoritative_tick`:29`prepare失败→fatal、`:32`prepared.commit；`candidate_commit`检查后无失败交换；`protocol/commit.rs:16`要求同次提交evidence；CivilSession事务提交屏障 | 主干已有；不引入catch_unwind掩盖panic；evidence确有生产入口，不靠最终账本伪推。G39轨迹冻结验收缺口不由此核销 |
| 同章 133–137：Rayon in_place_scope协调者保留宿主线程，嵌套worker限制 | `pipeline/stock_stream.rs:419`drive→`:430`实际rayon::in_place_scope、`:447`收已完成股票后`:449`续行 | 已有实际实现。原文主动登记Rayon worker内部嵌套协调者风险，没有承诺绝对无死锁；本批不凭“风险尚在”认定默认三个宿主新遗漏，也未实测活跃并行度 |
| 同章 139–144：临时目录、源码fingerprint、可比吞吐/RSS、真实runnable而非线程池数量 | `run-full-regression.mjs:499`build后fingerprint、`:501`拒绝源码漂移、`:596`执行后复核；simulation源码manifest与外部采样工具有入口 | 工具主干已有；未运行长矩阵，不能证明全证据或吞吐达到目标；target正式编译缓存范围由ADR0028明确保留，不机械要求所有Cargo缓存迁进.tmp |
| 同章 145–148：两workspace、WASM适配不污染核心 | Cargo/pnpm清单及WASM packageManifest→engine | 已有，未新增依赖 |
| §6 150–160：intent经过统一host/session再Redux、任步显错 | App→useTradingCommands→submitIntent→worker/remote/tauri→protocol coordinator→Redux投影；失败notice/错误页 | 实际生产链存在；图中旧delta称谓D01；入队≠订单受理，仍须看权威确认，不把入队成功称成交 |
| §7 163–171：已决语言、包管理、monorepo、联机协议 | 清单对应现行manifest及适配器；REST+WS远程路线明确 | 已决路线，非新增待做；未开放或推翻ADR |
| ci-build-fixes引言 1–4：2026-10-02平台耗时修复，保持10秒/5分钟 | 正式runner常量与runBoundedCommand限制已存在 | 实现记录，真正Windows/macOS耗时改善需真实Actions结果；本文静态审计不宣称平台超时已根治 |
| CI修复 6–7：全回归只移除重复Node，仍进程外10秒supervisor | `run-full-regression.mjs:73`正式Web step直接 `run-web-tests.mjs --internal-worker`、env授权；`:592` timeout=min(10000,剩余)、`:593` bounded phase runner；独立webCLI `run-web-tests.mjs:146`另起监督worker | 已接正式回归入口；不是仅test helper模拟，也没有跳过Web套件或移除监督 |
| CI修复 8–9：SSR/compiler独立分片、其余多核均分 | `run-web-tests.mjs:63`最多8/CPU/文件数分片；`:65`识别workspace-grid、`:70`独立第一片、`:71`其余均分；`:105` Promise.all并行、`:114`10000 case timeout | 已接默认pnpm test及完整回归。单片内部concurrency1用于隔离，不等于整批单核；CPU=1合法硬限制无可并行资源，不列漏实现 |
| CI修复 10–13：macOS预编译10.13、固定Tauri默认、非ARM支持承诺 | `.github/workflows/distributions.yml:234`Desktop预编译条件、`:237`macOS env10.13、`:238`真实compile-only脚本；`:223`固定tauri-cli-v2.12.1；tauri.conf未覆盖macOS最低版本 | 已消费到正式workflow；prepare不发制品且bundle后续独立。未推导arm64能运行10.13，不要求本批更改最终最低版本 |
| CI修复 15–19：官方依据、升级/配置变更时一致性门禁 | `distribution-macos-cache.test.mjs:5`检查正式workflow预编译env与config最低版本，`:13`拒绝与10.13不一致 | 契约短测存在；不属于生产runtime接线漏。仅测试源码已读，未重跑；引用官方代码日期不是本批重新联网核查 |
| CI修复 21–23：沙箱输出限制不改断言、静态验证非真实CI耗时证据 | runner显式失败传播，文档说明按真实Actions核实 | 证据要求，不能当作代码尚欠“自动获得沙箱外执行”功能；本批未访问真实CI也不报告通过 |

## 现行范围优先与文档漂移

### D01：架构的delta命名仍是旧协议描述

- 原文 `architecture.md:33`、`:158` 说HostUpdate只baseline/delta。
- 当前 `host-update.ts:11` 类型是baseline与protocol，protocol携带带generation的完整EngineUpdate；`normalize.ts:38`分TickBatch/CivilUpdate，protocol reducer检验seq/tick再消费。
- 同一篇 `architecture.md:124` 已承认独立CivilUpdate。新版UX、ADR0017及现行代码契约优先，因此是名称/说明漂移，不缺“再实现一个delta适配器”。允许替代权威runtime_snapshot的兼容范围不能据旧名字删除。

### D02：Stage2数据库与多人后台不属于当前隐含待办

- 原文 `architecture.md:65`列出服务端数据库及权威联机阶段；`:82`也把DB列作可替换存储选项。
- ADR0027明确本轮只复用会话JSON与既有远程模式，不新增多人同局/账户体系；当前Server存活会话与客户端凭据已有。
- DB没有生产实现不等于这些描述完全实现，但它是未来范围，不列现行代码遗漏；已存在远程连接缺口G01–05继续保留。

### D03：临时缓存说明与正式编译target缓存的作用域

- `architecture.md:139`说所有验证副本/进程临时目录/缓存放.tmp；ADR0028现行要求Cargo编译目录 `target/build-cache/<product>` 可缓存，禁止整target把旧发布产物带回。
- `.github/workflows/distributions.yml:249`只缓存对应product目录，实际发行产物在`:252`本轮独立package目录；本文没有发现以旧制品冒充新run。
- 不能把正式Cargo cache位置当漏实现搬迁；架构应明确验证临时副本与受控编译缓存的边界。

## 候选检查与反证

- 候选“README诊断命令没加build-std故不可用”：`.cargo/config.toml:16`显式配置unstable build-std，nightly固定，反证成立，撤销候选；尚未跑真实调试WASM构建。
- 候选“正式frontend脚本只打印/没真正复制WASM”：`frontend-build.mjs:73` cp且commandRunner执行build plan，外部deadline监督存在，撤销候选；dry-run不能当真实制品。
- 候选“COOP/COEP只在Vite开发存在，生产缺头”：`server/web_ui.rs:106`正式中间件给所有response加头，`static_router:103`真实安装；Pages则通过 `main.tsx:8` / `pages-isolation.ts:12` 模式限定启动门禁在renderApp之前完成。撤销候选；普通远程HTTP缺secure context明确报错是批准边界。
- 候选“CI优化只测试helper、不进真实回归”：完整回归正式步骤直接internal-worker，外围bound仍10秒，workspace-grid分片在默认runner policy实际调用，撤销候选。
- 候选“发布build-only漏跑测试/lint”：ADR0028明令发布与手动产品构建不跑这些套件，独立CI维护入口仍在；不能恢复到发布链作为修复。
- 候选“macOS预编译未安装最低版本环境”：正式reusable workflow的prepare step已设10.13，当前配置不覆盖默认，撤销候选；真实cache命中与后续平台wall time未实测。
- 既有G01–05/G18/G19远程与宿主交付、G20新局seed、G26lint门槛、G31图表引用隔离、G37诊断实际订单关联、G39受理轨迹验收均不能由本篇README/架构宣称完成；本组不重复编号。

本批三篇251行逐章追踪后，未确认新增现行生产代码缺口；文档漂移3项，候选经实际消费反证撤销6项。首次冷构建、诊断包、浏览器安全上下文、三平台真实CI耗时与完整领域回归仍是未执行的验收边界，不能写为通过。
