# sweep47：旧公司问题、技术栈与工作状态全文核对

## 范围与全文记录

- 主控指定 `b76ece3` 产品基线，按 `.worktree/implementation-reaudit` merge HEAD 同产品源码审查。
- 已读取根 `AGENTS.md`、`docs/principles.md`；只新增本工作记录，不改产品、测试、Git，不启动长测试。
- `docs/superpowers/specs/2026-09-13-company-information-problems.md`：95 行，连续读取1–95。
- `docs/tech-stack.md`：64 行，连续读取1–64。
- `docs/work-status.md`：396 行；首次批量输出截断，因此补分段连续读完1–140、141–273；274–396在初次输出中完整读取。合计555行，未把标题／搜索命中当全文。
- 旧总账对照 `agents/implementation-audit/implementation-audit-2026-10-02.md`；本轮未运行测试，文档内历史成功／失败与远程Actions结果不冒称本轮重新验证。

## 旧 problems 逐章状态

| 原文章节／行 | 当前判断 | 代码证据或后续修正反证 |
|---|---|---|
| Todo4 documentation，7–13：Wayland截图／8MiB远程gate | 视觉属于验收债；旧8MiB门槛已替代 | `apps/server/src/routes.rs:41` MAX_LOAD_BODY_BYTES = engine上限＋1MiB；`packages/engine/src/session/persistence.rs:944` engine上限512MiB；`apps/server/src/lib.rs:109`将同上限装到load router；`routes.rs:809`再做body、深度与decode验证。不能把“8MiB成熟存档不能上传”当当前新遗漏，也不以512MiB宣称无限规模。 |
| Cross-host period，17–29 | 已修，resolution优先 | `packages/engine/src/company/query.rs:493`用共享period_end_date输出YYYY-MM-DD，`information/publication.rs:181`统一期末；Server／Desktop直接用同一DTO，WASM桥无日期算术。此前sweep29已追完整caller。 |
| Task35 native blocker／resolution／retry，31–45 | 旧环境阻塞已被37行及90行后续记录核销；截图限制仍在 | 不重新安装依赖、不声称当前容器运行了GTK/Wry。诊断生产源码与feature已有，真正IPC与像素证据以现有后续验收记录为准；没有缺少代码的独立结论。 |
| Task34 chart artifact，47–52 | 未证实当前实物仍复现，保留视觉验收边界 | 文档要求将黑TV水印残片归chart rendering；本轮未做pixel捕获，不能凭旧图或搜索无命中判当前存在同样artifact，也不能无依据移除第三方图表标识。旧总账已保留移动／Wayland视觉证据债。 |
| Controlled date no-silent-fallback，54–60 | 已修 | `apps/web/src/App.tsx:149`初始化来自sessionSetup；`:502`／`:647` value直接startDateDraft；`apps/web/src/app/useSaveCommands.ts:212`parseStartDate后才新局。空字符串没有通过展示fallback伪装成默认日期。 |
| Task36 source scope／clock issue／resolution，62–86 | 原授权阻塞和午休权威clock缺陷已修；真实订单关联另有G37 | `packages/engine/src/session/observation_clock.rs:4`权威source包含开盘竞价、连续、尾盘；`:27`连续交易秒达到7200加5400秒午休。`decision_chain.rs:74`和`session/causal.rs:28`消费同一instant；`decision_chain.rs:3389`午休边界测试。不能只修diagnostics投影伪造时点，也不能把已有G37因果关联债混同午休缺陷。 |
| Resolve-blockers Task1／Todo2 clean checkout，88–95 | 历史验证局限，非新增产品功能 | 文档自己明确旧native blocker解决、真实IPC／Wayland另有lane，未提交和缺Node影响当批执行证据。不复活原Task状态或旧固定环境，不伪造clean-checkout通过。 |

## 技术栈逐章与各工程承诺

| 原文行／条款 | 状态 | 最新caller或边界 |
|---|---|---|
| 9–22：React、Vite8、TS、RTK、Tauri2、Rust/WASM、Rust Server、pnpm、Node test、ts-rs、Playwright | 已选且生产配置存在 | `apps/web/package.json` dependencies/devDependencies对应React18、Vite8、RTK、Playwright1.63；根`package.json`固定pnpm11.19与Node>=24.18；`apps/server/Cargo.toml:24`Axum、`:31`tokio；`packages/engine/Cargo.toml:55`ts-rs workspace；配置存在不等于每个平台原生运行已验收。 |
| 23：CI warning视为错误 | **已有G26，仍缺Webwarning失败门槛** | `apps/web/package.json` lint仍裸oxlint；`.github/workflows/ci.yml:218`调用它；RustCI clippy用-D warnings。新决定将CI变手动开发诊断（`ci.yml:4`／`:7`），不要求恢复push/PR/tag自动CI或发布lint。 |
| 29–37：纯engine、共用crate、固定nightly及多线程WASM构建 | 已有接线 | `.cargo/config.toml` atomics／bulk-memory／mutable-globals、shared/import memory、TLS exports与build-std；`scripts/wasm-build.sh`／`.bat`委托frontend-build；`scripts/frontend-build-plan.mjs:16`实际wasm-pack --release -Z build-std，`:17`固定nightly-2026-09-05，`:18`／`:21`前后都check-wasm-threading。成员没有另建并行技术路线。 |
| 39–45：后端可选、Axum、engine权威与RTK结果投影 | 已有 | 三宿主均运行engine crate／binding；`apps/web/src/App.tsx:217`统一ProtocolCoordinator消费权威更新；UI不另算成交、T+1、费用或恢复账本。仍不能核销现有远程协议G01–G05。 |
| 49–64：覆盖率待ADR、选型依据与未定不引入 | 待决／流程，不计当前缺代码 | 没有覆盖率百分比或ADR就不要求造硬阈值，和`work-status.md:389`同边界。本轮不安装LSP、技能或新技术栈。 |

## work-status 全部章节与条款状态

| 章节／原文行 | 状态与最新生产证据 |
|---|---|
| 页头1–4 | “已实现／未验收／撤销／待决定”是审计分类原则，不以checkbox替代真实caller。 |
| 三平台分发6–95 | 当前构建／打包存在：`scripts/build-targets.mjs:92`原生Desktop --lib预热、`:96`无签名Tauri打包、`:101`原生Server目标；`scripts/package-distributions.mjs:196`要求MSI/NSIS或DEB/RPM/AppImage或DMG，`:238`要求mac app；`:165`ZIP/tar.gz，`:181`macUTF-8 ZIP；`:107`检查相对symlink范围，`:54`输出目录边界。文档多轮失败以8–24最终结果优先，但本轮未查gh或复验实物，不能扩张为GUI安装／签名／游戏旅程。 |
| 分发并发／deadline／独立复核14–17、78–95 | runner代码存在，历史数字不重报通过：`build-targets.mjs:14`300000、`:21`available jobs默认、`:272`记录jobs；`frontend-build-plan.mjs:8`显式CARGO_BUILD_JOBS／RAYON_NUM_THREADS。旧“预热没成功”已被后续阶段拆分记录覆盖，不以分钟构建当普通单测。 |
| 运行时宿主／四目标97–115 | 已实现本地／远程选择与构建分离。`scripts/build-targets.mjs:101`server/webui-server feature区分；`:323`删旧VITE_ENGINE_HOST／VITE_REMOTE_BASE_URL／VITE_REMOTE_TOKEN。`apps/web/src/host/startup-policy.ts:26`安全上下文／cross-origin isolation／SAB不满足显式失败；不降级或自动切宿主。静态服务、纯server无Node入口和部署包代码已有，旧总账R19／部署组继续适用。 |
| 运行时目标验证117–138 | 所述历史20/14/100/51及Linux实物证据、旧Tauri CLI缺失、diagnostics TS2769均需区分时间／范围；后续分发已补native打包，不再把134行当当前“桌面从未构建”。没有本轮完整回归或跨平台E2E结论。 |
| 合并失败修复140–170 | 竞价五股单股无成交合法，测试已分代表性单股与多股统计；不据旧失败造对手盘、补钱或恢复共同V。Clippy Box Fatal保持序列化原HostFailure，根`package.json`仍有fmt／workspace clippy门禁；此批未重跑。ADR0024明确现金池可减，未增加投资者现金循环。 |
| 全文盘点及ADR0025推进172–188 | 当前最小日终候选、独立目标、替换屏障及严格新格式已实现，sweep11逐条追完整链。原A01–A11已实现主干不能核销旧G局部缺口；B20旧日内买单资金问题已由日级档契约消解，不新增补钱／自动撤单。 |
| 启动恢复190–204 | `useSessionHostLifecycle.ts:90`单次InitialSaveSource读，`:96`setup与`:97`seed来自档，`:105`推进前load；`ProtocolSession::restore`拒非完整日结及活动订单。旧坏档fallback、日内母单漏检都已有修正。Worker nextGeneration异常响应单调性另已在sweep29候选C29-01，不因“保存generation已修”核销或重复新建。 |
| 定向验证206–218 | 历史短测、编译、真实WASM门禁及未完整TDD如实分类；`scripts/run-web-tests.mjs:114`case 10000ms，各独立进程分片，单进程concurrency=1不等于整体单核；全量另由`run-full-regression.mjs:467`／`:470`明确记录CPU jobs与deadline。没有本轮执行这些gate。 |
| A05精简220–238 | 必要事实DTO与恢复重建已有：`packages/engine/src/session/persistence/v2.rs:632`保留实际filled_value／audit；公共日级档另拒日内state。234行旧部分成交失败不能据此认定当前生产坏。 |
| Continuous部分成交恢复240–271 | 后续根因更正优先：`packages/engine/src/session/pipeline/continuous_tick_transaction_tests.rs:521`现行用例，`:537`／`:559`／`:584`／`:587`用snapshot_inner(true,true)取全账户。旧UI snapshot只玩家造成测试错，不能改生产母单观点／存档。公共ProtocolSession日终入口与内部GameSession恢复是不同用途。历史六次首绿不称本轮新修复／本轮通过。 |
| 二次补漏273–290 | `session/institutional_behavior.rs:58`读取必填本人账户risk latch；`decision_chain/roots.rs:465`本人root消费经历；保存generation固定候选见sweep11。`components/price-chart-indicators.ts:13`日K读取daily OHLC，`:27`完整结果再按窗口slice，`:39`量能按日时轴；`App.tsx:660`按snapshotGeneration重挂DEV，`dev/NpcDecisionInspector.tsx:37`／`:41`／`:44`／`:47`旧成功、失败与finally均有gate。不把旧G37实际订单关联核销。 |
| 二次补漏验证／许可副作用292–311 | 当批红绿、Rust/Web人数、WASM exports、缺模块首失败、许可证恢复是历史事实证据与工具副作用；未运行、未修改LICENSE，不补写TDD或规模结果。静态源码检查不足以证明长期内存／吞吐达标。 |
| 旧记录核销313–324 | 任务27WIP已过时；任务37/38/40/41/42与F门禁完整验收仍缺；task39[x]有历史规模实测但独立闭合未证；旧escrow排序／全worker字节一致由新契约替代；未填模板不作任务。仍由旧G39及验收章节覆盖，不重复生产功能。 |
| 9月30推进325–337 | CLI写出、移动SSR/行为短测等已有工具／测试；329行移动Playwright未执行是当批验收债，非“没有移动端实现”。后续真实浏览器证据应按具体场景核对，不能泛称全部未验或全部通过。 |
| 机构策略与真实经历341–369 | 本人cost／风险／adverse、本人净获利与费用、失败身份、信心消费及恢复权威已有：`session/pipeline/institutional_experience_projection.rs:9`真实收据经历，`session/pipeline/account_settlement.rs:3`调用投影，`session/decision_chain.rs:172`经验反馈、`roots.rs:465`生产root消费；`session/persistence.rs:1231`起校验policy／experience事实。不重开统一止损／上限／留底，不因机构链实现核销散户G07/G08或中期G09。 |
| 私有会话授权371–377 | Server私有token主干及DELETE授权存在，但浏览器WS query token/header不一致与speed GET漏凭据仍为旧G01/G02；“服务端守卫已实现”不能证明浏览器每个caller已正确带token。账号／TLS／多人权限仍未来，不假装已有。 |
| 数据范围379–383 | 360根负日K及day0实际撮合已有，sweep11已逐条；`scripts/simulation/baseline-run.mjs:1391`／`:1416`现行synthetic policy；不恢复真实市场校准或授权等待。 |
| 不可冒充工作385–396 | 稳定多线程提速、真实宿主跨日E2E、覆盖率采集、公网／DB／TLS、签名／更新、多槽／云同步是未完成验收或待ADR；股东现金流明确不做。现有局部benchmark或源码不能核销整个表，也不把它们统称已确认代码遗漏。 |

## 新候选与反证结论

**本批没有确认总账之外的新代码漏项。** 最容易误判的旧问题均已有后续修正：8MiB load限制、跨宿主period、午休权威clock、空日期展示fallback、部分成交母单恢复测试误取UI账户投影、当时Tauri native依赖阻塞。

仍成立而已覆盖的差距：Web warning门槛G26；浏览器WS鉴权／speed凭据G01/G02及其余宿主G项；因果诊断真实订单ID关联G37；稳定多线程与K7并发比较G39；公司／散户／公开报告局部G项。Task34黑TV残片、Wayland像素、native GUI和完整规模证据须先有当前实物观察，不能在未执行情况下升级新生产bug。

本报告只证明条款全文及已读caller可追踪，不能证明所有历史记录无矛盾、绝对无隐藏bug、无限规模或本轮全部验收通过。
