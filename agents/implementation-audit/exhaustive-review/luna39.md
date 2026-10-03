# luna39：README／架构／CI 修复说明全文复核

## 范围与基线

- 按要求连续读至 EOF：`README.md` 248 行、`docs/architecture.md` 171 行、`docs/ci-build-fixes.md` 23 行，共 442 行；阅读不是摘录代替。另读根 `AGENTS.md` 和 `docs/principles.md`。
- 产品基线是 `08e4fc7`，当前审查 worktree HEAD `a7c7ce3` 是包含该产品提交的 merge（`08e4fc7` 为其祖先）。
- 对照最新 ADR-0027、ADR-0010、ADR-0017，当前 web 启动/host 实现、Vite 和 Pages isolation、Rust/Axum 静态服务、构建计划、CI workflow/cache caller；也复核先前 S18/S19 文档结论。未运行测试、编译或长验收；未修改产品代码或 Git 状态。

## 逐章覆盖矩阵

| 文件原文范围 | 当前实现/决定对照 | 复核结论 |
|---|---|---|
| `README.md` 1–15：项目简介、ADR-0017与验证边界 | 现行 README 保留性能、确定性、宿主验收的独立门禁和 panic 边界；架构 §Escrow 108–148、ADR-0017及已有独立审计一致。 | 旧结论仍成立；不可把运行采集或 worker 数称为确定性、提速、满核证明。 |
| `README.md` 18–50：愿景、Stage 表、当前能力 | `App.tsx:700–739`生产/开发默认等待选择，E2E才初始化 WASM；按启动选择调用 WASM Worker、remote 或 Tauri。README:47–49的统一 `EngineHost` 和能力概述与实际模式相符。 | Stage 3 表中“后端不需要”应理解为可本地运行；README:78同时明确 Desktop 可选远端，不构成互斥承诺。 |
| `README.md` 51–70：开发、WASM准备、宿主选择与 env 初值 | `scripts/wasm-build.sh`/frontend orchestration生成 WASM package；`App.tsx:701–728`只在 DEV 将 `VITE_ENGINE_HOST`、`VITE_REMOTE_BASE_URL`填入启动选择界面；`compilerEnvironment`在产品构建剔除这些 env。`startup-policy.ts:26–58`明确本地浏览器不满足安全上下文/隔离/SAB时拒绝启动，不静默降级；remote基址校验。 | 开发 env“仅作为表单初值”准确。`pnpm build`所需已有 WASM 产物与脚本计划相符。README指向 build-and-deployment 说明协议/隔离部署限制。 |
| `README.md` 71–135：产品目标、静态 WebUI、前置条件、jobs、打包和文档链接 | `scripts/build-targets.mjs:63–107`规划 `desktop/webui/webui-server/server`；`apps/server/src/deployment.rs`通过 `web-ui` feature 和 `--services`限定服务面；纯 server 构建排除 UI。README:108–109正确区分静态部署机与访问者浏览器的本地 WASM Worker。 | 与 ADR-0027及调用者一致。前端构建、Rust二进制和部署运行依赖区分清楚。 |
| `README.md` 137–223：Linux桌面依赖、headless验证及3×3矩阵 | Tauri GTK/WebKit依赖与运行环境说明为条件性指引；构建矩阵平台限制与 desktop build planner 一致。 | 属平台/环境边界，不宣称本轮已构建/安装；未发现新增产品承诺。 |
| `README.md` 227–248：贡献和许可证 | 与根 AGENTS、ADR-0007相符。 | 全章核对，无新候选。 |
| `docs/architecture.md` 1–35：Engine中心、Rust、多宿主与统一 `EngineHost` | ADR-0010的 baseline/delta/宿主能力契约仍在；`EngineHost`实际承接三个宿主。 | 主要架构主张成立；第6节的具体调用序列另见新候选。 |
| `docs/architecture.md` 37–55：分层及依赖矩阵 | workspace成员和 engine/app crate 边界仍符合核心不依赖 UI/I/O 的方向。 | 属架构约束性描述；本次限定审查未发现跨壳代码依赖反证。表中“表现层直接碰存储/网络”是理想边界，不可误读为已完全落实到所有组件。 |
| `docs/architecture.md` 57–75：后端可选、模式表、ADR-0027部署 | ADR-0027及 `apps/server/src/deployment.rs`确认 `--services webui|server|all`；`web_ui.rs:97–120`对静态响应加 COOP/COEP 和 nosniff；Vite dev/preview也加隔离头。Pages没有源站隔离头时由 `pages-isolation.ts:12–43`安装 Service Worker、首次刷新再检测。 | 静态服务/服务面说明准确，但 Stage 2 模式表把“服务端数据库”列作当前已落地存储存在陈旧/歧义（下列候选）。隔离机制未在本节解释，属于文档交叉引用/完整性候选，而非代码缺陷。 |
| `docs/architecture.md` 77–83：状态、持久化和校验 | principles §4一致。客户端实际统一存储仓储；server代码可见actor会话及显式save/load endpoints；server Cargo依赖无数据库驱动，未找到DB实现。 | “文件或DB”可作为可替换架构选项，但与“每次读取”及文档总述的当前完成态需区分已实现/规划。 |
| `docs/architecture.md` 85–106：目录结构与依赖 | 当前 Cargo workspace 使用 `packages/engine`, `packages/engine-gpu`, `apps/web-wasm`, `apps/server`, `apps/desktop/src-tauri`；pnpm管理 `apps/web`。 | 路径与 workspace段落正确。图中 `game-engine`是概念名而实际 crate/package 是 `engine`，不影响概念但建议后续统一术语。 |
| `docs/architecture.md` 108–148：Escrow tick、失败、事件、Rayon、性能与workspace | 当前代码和 ADR-0017对照后，段落的承诺边界与现有 S18 复核吻合；已有文档明确 nested-RayOn worker caller 可能耗尽池的限制。工作区/缓存隔离说明是验证约定。 | 旧架构风险结论仍有效且已披露，不能升级为“已复现死锁”；此轮没做压力/性能复测。 |
| `docs/architecture.md` 150–161：买入数据流 | 实际 adapters（WASM、remote、Tauri）向应用提供 protocol update；实际协议数据包含 `TickFrame`/`CivilUpdate`/`TickBatch`、`HostUpdate` baseline/delta。低层 `GameSession::step`仍返回事件，但 UI 的当前宿主路径不是直接接收它。 | `GameSession.step() → Event[]`将低层 engine API和生产宿主协议拼成一条简化链，容易误导目前数据流（候选见下）。 |
| `docs/architecture.md` 163–171：决策索引 | Rust、Cargo/pnpm、REST/WS决策仍有对应ADR。 | 无新候选。 |
| `docs/ci-build-fixes.md` 1–23 全文：门禁、Web worker/cache分片、macOS部署目标、证据限制 | `run-full-regression.mjs:73–78`在进程外普通期限监督下调用Web internal worker；`run-web-tests.mjs:105–139`按CPU分片、隔离执行且给每case 10s timeout。`distributions.yml:234–250`准备Desktop原生compile cache、macOS阶段设 `MACOSX_DEPLOYMENT_TARGET=10.13`，后续调用同一目标构建；短测 `distribution-macos-cache.test.mjs`检查workflow设置与Tauri config。 | 现行调用者与修复说明相符。历史“必须以对应 Actions 实测判断耗时”是诚实的证据边界，不代表本轮已取得新的平台耗时证据；普通提交无自动CI依据仍服从ADR-0028。 |

## 旧结论复核与新候选

1. **旧结论保留（S18）：nested Rayon 风险是已披露边界。** 当前 `docs/architecture.md:133–137`仍明确只解决从宿主/测试线程进入时协调者占满池的问题，嵌套 worker caller 风险未被声称消除。当前有限静态复核不能声称发生死锁，也不能把旧说明当“已证明安全”。
2. **旧结论保留（S19）：CI修复属于受期限约束的实现细节。** 本轮核实Web internal worker并未绕过外部期限；分片实际执行于多个Node进程；macOS target由分发workflow在预编译步骤设置，固定Tauri CLI/config短契约仍是其依据。短契约不证明平台超时已在本轮复现或消除。
3. **候选 D1（文档事实，建议同步）**：`docs/architecture.md:61–66`标题上下文是“如何实现”且上方明确写“描述当前已落地结构”，但 Stage 2“权威后端”行把存储列为“服务端数据库”。`apps/server/Cargo.toml`没有数据库依赖，server routes/actor为内存会话与客户端存档 API，代码检索未见DB/迁移实现。若该行描述目标而非现状，应明确标成规划/未来选项；否则应改为当前实际会话/存档边界。反证：原则层可以讨论可替换 DB，且行文可能是目标模式，不应将“未实现DB”扩大成 server 功能错误。
4. **候选 D2（数据流陈旧，建议同步）**：`docs/architecture.md:154–159`称适配层交付后直接 `GameSession.step() → Event[]`，再统一交付 `HostUpdate`。生产 caller通过 `EngineHost`进入具体宿主协议；engine protocol有 `TickFrame`、`CivilUpdate`等，WASM/remote/Tauri adapters解析或转换更新后才给UI。`GameSession::step`确实存在并返回 `Vec<Event>`，所以它是有效的简化 engine API；问题在把它写成当前跨宿主的UI数据流，漏掉统一协议内部的更新形态。应改用 `EngineUpdate`/`TickFrame`等实际边界，或显式标注为省略协议层的概念图。反证：架构图不是逐函数调用时序，`HostUpdate`语义确实被UI统一消费；属于表述精度候选而非行为缺陷。
5. **候选 D3（隔离架构遗漏，低优先）**：架构 §3写浏览器本地局在 WASM Worker，但没写当前实现强制需要 HTTPS/安全上下文、`crossOriginIsolated`、`SharedArrayBuffer`，也没标明服务端COOP/COEP或Pages Service Worker路径。README:134–135已有完整部署文档交叉引用，运行时 `startup-policy.ts:26–33`会拒绝缺条件且给出响应头要求；因此不是静默回退或当前不可用证据。考虑到本节定义当前Web Worker架构，这一前置条件适合简要列出或加明确链接，防止“有 Worker 即可本地运行”的印象。

没有发现交易语义变化；候选均为架构文档准确性/完整性问题，需由主审计决定是否纳入文档改动。本轮没有直接修改这些文件。
