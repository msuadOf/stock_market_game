# Sweep 12：标签发布、构建部署与 Actions cache 全文复核

## 基线与读取范围

- 审计源码基线：`b76ece39b3a1635adde52da07375607f19b56ecc`。
- 实际读取 worktree HEAD：`4ad5a2e298d086024e6b0069b98b4a6b195a0a01`；`git diff --name-only b76ece3 HEAD -- docs scripts .github apps` 无输出，下面涉及的文档、workflow 与产品源码和审计基线一致。
- 已读 `AGENTS.md`、`docs/principles.md`、相关 `docs/open-questions.md` 与 `docs/architecture.md`，本报告只写审计工作文件，不修改构建链路或 A 股交易语义。
- 三份分配文档均连续阅读全文：`docs/decisions/0028-tagged-release-and-static-pages.md` **103 行**、`docs/build-and-deployment.md` **335 行**、`docs/actions-cache.md` **60 行**，合计 **498 行**。读取时拆分长输出，后续重新读取缺少的片段，没有仅凭关键词搜索判定全文覆盖。
- 现行决定是 2026-10-03 的 **build-only**：发布及手动产品构建不运行 CI、测试、lint、Clippy 或 smoke，不恢复旧门禁，不将暂不签名、公证、GUI 安装验收归为代码遗漏。
- 本轮未调用 GitHub API、未部署、未编译、未运行长测试或完整回归。做了只读源码核对及一次立即结束的 Node 路径表达式检查；测试文件仅作为已有覆盖证据读取，不宣称本轮执行通过。

## ADR-0028 章节与原文逐项对应

| 原文章节 / 行号 | 当前代码证据 | 判定 |
|---|---|---|
| 状态与日期 `:3`，2026-10-03 发布仅构建 | `.github/workflows/release.yml:32` 直接调用 `distributions.yml`；`.github/workflows/ci.yml:6` 仅 `workflow_dispatch` | 已实现；旧标签曾执行 CI 是历史事实，不是当前欠实现 |
| 决策 `:9`，普通 commit/PR 不触发；有效版本 / 测试标签 | `.github/workflows/release.yml:3`；`scripts/release-policy.mjs:4`、`:15`；七产品 workflow 与 `ci.yml` 均只有手动/复用入口 | 已实现；有效性脚本在分发前执行 |
| 决策 `:15`，十组制品、manifest、大小、SHA-256、draft、上传后核验、ASCII 外部名称 | `.github/workflows/distributions.yml:141`、`:262`；`scripts/publish-release.mjs:13`、`:79`、`:89`；`scripts/package-distributions.mjs:12` | 已实现；十组身份去重、目录与文件清单核对、符号链接拒绝和远端 digest 复核均有实际逻辑 |
| 决策 `:20`，拒绝标签移动 | `scripts/publish-release.mjs:83` 初次核对；`:109` 上传及资产核验后再次核对；`:113` 才公开 | **G27 继续核销**；不是新缺口，也不声称 GitHub 提供原子标签锁 |
| 决策 `:25`，Release 携带源码 SHA，不自动改产品版本 | `scripts/publish-release.mjs:68` 写 `release-source.json`；`:86` 发布说明明确产品版本保留源码值 | 已实现；没有要求每一份 native manifest 新增 commit 字段 |
| 决策 `:28`，七按钮、固定 SHA、共用实现、手动不发 Release | `.github/workflows/build-windows.yml:9`、`build-linux.yml:9`、`build-macos.yml:9`、`build-server.yml:9`、`build-web.yml:14`、`build-webui-server.yml:9`、`build-all.yml:9`；`distributions.yml:81`、`:199` 使用 `github.sha` 的本轮 artifact | 已实现；checkout 默认取本次事件固定 ref/SHA，没有执行中追分支 HEAD 的自定义逻辑 |
| 静态运行边界 `:39`，静态 WASM、本地/远程选择，不部署 Node/Rust | `distributions.yml:90`、`:107`；`apps/web/src/app/useSessionHostLifecycle.ts:204` 分发 WASM/Tauri/RemoteHost；`scripts/package-static-web.mjs:8` | 已实现 |
| 静态运行边界 `:41`，项目子路径与用户/组织根站点区分 | `distributions.yml:88`、`:89` 用 `.endsWith('.github.io')` 决定 base | **有新候选 S12-01**：误将任意同后缀项目仓库识别为根站点 |
| 静态运行边界 `:44`，限定 scope SW、只同源静态请求、保持 request/response、不缓存 | `apps/web/public/pages-isolation.js:5` 同源、GET、scope 与静态资源白名单；`:12` 原 request 转交 fetch；`:17` 保持 body/status/statusText/headers，增加隔离头 | 已实现；API 与其他仓库路径未列入拦截白名单，没有 CacheStorage 实现 |
| 静态运行边界 `:47`，App 载入前一次重载、失败显式、运行中不刷新 | `apps/web/src/main.tsx:7` 先隔离后动态导入 renderApp；`pages-isolation.ts:14`、`:24`、`:36`、`:41`；`main.tsx:24` 展示错误 | 已实现；更新 SW 的 `skipWaiting/claim` 没有刷新运行中 App 的 handler |
| 静态运行边界 `:52`，独立定向 smoke、HTTPS/远程约束 | `scripts/smoke-pages.mjs:10`、`:35`、`:42`；产品 workflow 没有调用该工具；`apps/web/src/host/startup-policy.ts:26` | 已实现；真实线上验收仍需单独证据 |
| 缓存、权限与验收 `:59`，只读构建、独立发布/部署/删除权限、Pages queue max | `distributions.yml:38`、`:145`、`:265` 构建叶 job 只读；`release.yml:44` 发布写权限；`build-web.yml:28` Pages 权限及 `:32` queue max；清理叶 job 独占 actions write | 已实现；调用 reusable workflow 的父 job 传 actions write 是供其清理叶 job 使用，不等于构建叶 job获删除权限 |
| 缓存、权限与验收 `:66`，成功标签退休、失败保留、活动 run 暂缓、补退休、预算 | `release.yml:76`、`:94`；`scripts/prune-actions-cache.mjs:84`、`:105`、`:132` | 已实现；10GB 保护项超预算显式报错，不承诺每个时刻严格低于容量 |
| 缓存、权限与验收 `:73`，compile-only 白名单隔离旧产物 | `distributions.yml:59` cache-targets false、显式目录及新 prefix；`:187` 只缓存 native build-cache/依赖 | 已实现；没有整 target 回填 |
| 缓存、权限与验收 `:78`，provider alias 查询真实同名分支 | `prune-actions-cache.mjs:24`、`:57`、`:115`、`:120`；`:89` 限制当前成功标签 guard | 已实现；每次 readPlan 都重新查询分支，不靠名字猜删 |
| 缓存、权限与验收 `:84`，Desktop 库预编译/打包独立 deadline、CI 保留诊断 | `distributions.yml:234`、`:239`；`scripts/build-targets.mjs:90`、`:343`；`ci.yml:184`、`:200`、`:212`、`:218` | 已实现；不恢复发行测试链路 |
| 缓存、权限与验收 `:88`，真实构建/部署/回归分开报告，不改 A 股 | `docs/build-and-deployment.md:273` 记录各历史基线和未覆盖验收；当前发布 job无领域参数修改 | 明确范围与证据限制，不列新代码缺口 |
| 技术依据 `:95` | 已有官方链接为设计依据；本轮读取且追对应实现，未重新联网验证外部服务状态 | 依据章节，不是额外产品功能 |

## build-and-deployment 全章节核对

| 原文章节 / 行号 | 当前代码证据 | 判定 |
|---|---|---|
| 引言 `:3`，原生生产服务与统一 UI | `apps/server/src/main.rs:1`、`:30`；`apps/server/src/deployment.rs:176`；UI host lifecycle `:204` | 已实现 |
| 构建目标 `:8`，共享前端入口、固定安装、WASM 线程检查、TS/Vite、deadline | `scripts/frontend-build.mjs:34`、`:58`、`:78`；`scripts/frontend-build-plan.mjs:12` 至 `:24`；`scripts/check-wasm-threading.mjs:4`；`scripts/check-web-release-wasm.mjs:15` | 已实现；源码 glue 与复制后 glue 均检查，最终 WASM 拒绝私有诊断导出 |
| 构建目标 `:26`，四目标、output、dry-run、拒覆盖 | `scripts/build.sh:4`；`scripts/build.bat:3`；`scripts/build-targets.mjs:16`、`:53`、`:132`、`:364` | 已实现；default output 和指定 output 均受直接子目录限制 |
| 构建机与部署机 `:60`，纯 Server 无 Node 分流、原生 target、Desktop fresh bundle、未签名、期限 | `scripts/server-build.sh:39`、`:45`、`:69`、`:78`；`server-build.ps1:76`、`:123`；`build-targets.mjs:86`、`:96`、`:262`、`:283`、`:343`；`apps/desktop/src-tauri/tauri.conf.json` Windows MSI 语言配置 | 已实现；GNU/PowerShell 是构建机依赖，runtime 依赖未被伪称消失 |
| GitHub 三平台未签名分发 `:91`，七入口、9 native 格、共享 UI、ASCII、版本、十清单 | 七个 build workflow；`distributions.yml:21` 默认三平台，`:147` 与 `:267` native/server matrix；`package-distributions.mjs:192` 检查平台 promised formats；`publish-release.mjs:13` | 已实现；所有产品先构建再打包，save-cache 的 always 不使失败制品继续上传 |
| 同章节 `:163`，归档独立、拒空文件/外链/覆盖、Unix权限/mac相对链接、prebuilt UI | `package-distributions.mjs:34`、`:64`、`:90`、`:99`、`:164`、`:248`、`:277`；`build-targets.mjs:273` 检查 prebuilt完整目录与发布WASM | 已实现；直接归档 CLI 本身不编译 |
| 静态 Web 与 GitHub Pages `:185` | `distributions.yml:85`、`:90`、`:94`、`:107`；`package-static-web.mjs:19`、`:24`；Pages SW 与 main 见上表 | 主链路已实现；站点 base 分类存在 S12-01；环境保护/HTTPS/CORS 是显式部署前置条件 |
| 部署服务三启动方式 `:216`，mode/default/bind/webroot/error/notfound | `apps/server/src/deployment.rs:23`、`:125`、`:157`、`:176`；`main.rs:31`；`web_ui.rs:127`、`:142`、`:157` | 已实现；仅 WebUI 不注册 app_router，纯 Server 请求 webui/all 显式失败；资源或 API 404不回 HTML |
| 启动时选择引擎 `:245`，本地/远程、生产env剔除、一次读档 | `apps/web/src/host/startup-policy.ts:14`、`:26`、`:36`、`:53`；`App.tsx:700`、`:726`；`useSessionHostLifecycle.ts:89`、`:204`；`save/session-replacement.ts:1`；`remote-host.ts:35` DEV-only override；`build-targets.mjs:321` | 所述启动能力已实现；RemoteHost整体协议边界由宿主专项复核，不把这一段作为其所有行为正确的证明 |
| 协议与浏览器约束 `:258`，HTTP/WS、外置TLS、安全上下文/隔离/SAB | `deployment.rs:201` help；`web_ui.rs:106` 响应头；`startup-policy.ts:26` 明确能力校验 | 已实现；证书、代理、账号/公网认证不在本轮需求 |
| 验证范围 `:273` 与七入口/线上实测 `:303` | 文档列出历史 commit/run、数量、hash、known limitation，并在 `:334` 明确旧标签云端 CI 与新 build-only差异 | 历史记录及验收边界；本轮未重新请求 GitHub，不增写线上已通过结论 |

## actions-cache 全章节核对

| 原文章节 / 行号 | 当前代码证据 | 判定 |
|---|---|---|
| 目标与手动命令 `:3`，10GB、系列最新、未知保护、默认dry-run、网络deadline | `prune-actions-cache.mjs:7`、`:8`、`:24`、`:79`、`:95`、`:129` | 已实现；retirement 是 ADR0028 的已决例外，不是误删未知缓存 |
| 删除前重读、活动run暂缓 `:20` | `prune-actions-cache.mjs:132`、`:141`、`:145`、`:150` | 已实现；活动run所有状态统一查询，GITHUB_RUN_ID只排除本轮收尾 |
| 自动清理 `:24`，CI/distributions/release尾job、权限、concurrency、不跑发行契约测试 | `ci.yml:20`、`:41`；`distributions.yml:114`；`release.yml:76`；各叶job actions write / queue max | 已实现；标签 run中的分发清理跳过，由release尾job等待Pages后处理 |
| 暂缓后的补退休、超预算/网络失败 `:39`、`:43` | `prune-actions-cache.mjs:105` 公开且含release-source的有效标签；`:135` 超预算保护项报错；`:158` CLI失败 | 已实现；API非事务与可能延迟是原文诚实边界，不需要恢复持久化回收状态文件 |
| 本轮实际清理 `:48`、代表性短测 `:56` | 原文为2026-10-02证据，测试源码现有；本轮只读 | 历史证据，不当成本轮执行结果 |

## 新候选 S12-01：Pages 根站点识别缺少 repository owner

- 原文：`docs/decisions/0028-tagged-release-and-static-pages.md:41`「项目 Pages 使用仓库子路径；用户/组织根站点使用 `/`」；`docs/build-and-deployment.md:194`「仓库站点按 `/<仓库名>/` 编译，根站点按 `/`」。
- 实现：`.github/workflows/distributions.yml:88` 仅提供 `REPOSITORY_NAME`；`:89` 是 `name.endsWith('.github.io') ? '/' : '/' + name + '/'`，没有读取或比较 owner。`:93` 将这个 base 直接传给 Vite。
- 缺少的边界：GitHub 根站点仓库名应对应其 owner 的 `<owner>.github.io`。owner 为 `alice`、仓库名为合法的 `docs.github.io` 时，它仍是项目站点；脚本却输出 `/`，期望 `/docs.github.io/`。同一逻辑同时影响手动 Web、All 和有效标签的静态包。
- 用户可见影响：该项目站点的入口会引用域名根部资源，SW也按错误 base 注册，静态资源与首次隔离引导不能按承诺的仓库路径工作。
- 只读表达式检查：运行现有三元表达式，对照上述契约，结果为 `stock_market_game → /stock_market_game/`（符合）、`alice.github.io → /`（符合）、`docs.github.io → /`（**不符合**，期望 `/docs.github.io/`）。该检查不构建、不改文件、不访问线上服务。
- 现有反证范围：当前仓库 `stock_market_game` 及文档记录的自定义域名子路径不触发此缺口，因此不推翻既有 Pages 实测通过事实；它也不是“任意自定义域名都必须自动根部署”的新增需求。
- 覆盖缺口：现有 `scripts/release-policy.test.mjs:22` 检查七入口和Pages权限，`scripts/pages-isolation.test.mjs:28` 检查已给定的scope；搜索所有相关测试未见非owner的 `*.github.io` 项目仓库分类用例。
- 建议归并：独立的 Pages base 输入分类遗漏，优先级可列 P2；修复需以 owner/完整仓库身份区分 root/project，并补代表性短测。本轮按审计授权只登记，不改产品实现。

## 已追查并排除的候选

| 初步疑点 | 反证与最终结论 |
|---|---|
| 发布前只查一次 tag SHA | `publish-release.mjs:109` 明确二次查询；`:113`才公开。G27保留核销 |
| 本次发布不跑CI/Clippy/smoke是欠实现 | ADR0028 `:3`、`:15` 和 build文档 `:143` 已改为build-only，源码遵守新决定 |
| native manifest没有commit，源码SHA不存在 | ADR要求每份Release额外携带SHA；`release-source.json` `:68` 已满足，本轮固定SHA且只下载当前run产物，没有要求所有native manifest复制commit |
| 十manifest没有逐个要求安装格式会漏发所有安装包 | 当前实际 producer `package-distributions.mjs:192`逐平台要求完整安装格式，`:277`受限归档；collector又核对完整当前run目录/清单/digest。不能凭人为重写全部producer契约虚报现行生产路径缺口 |
| 父 reusable caller 有actions write，构建可删cache | 编译叶job显式contents read；actions write是授权reusable清理叶job，未给构建叶job |
| 整target cache恢复陈旧分发目录 | 当前compile-only prefix/cache-targets false/目录白名单已修，native只恢复各自build-cache；历史污染不再作为新缺口 |
| tag清理延期后没有记住待删项 | `prune-actions-cache.mjs:105` 每轮查询公开Release并补退休，ADR明确不要求另写持久状态文件 |
| SW更新使日内App强制刷新或拦截RemoteHost API | Worker仅skipWaiting/claim，无更新刷新回调；fetch有同源scope和静态白名单，API路径不在其中 |
| 手动Server装了Node前端工具 | server job用native步骤锚点，但所有setup-node/download UI/Tauri步骤都受product条件保护；真正build在shell/batch分流，runner自带Node仅归档工具使用 |
| 暂不做签名、MSI/DMG安装、默认两万NPC性能是新代码缺口 | 正式文档明确为独立验收/非本轮范围，不纳入本候选 |

## 收口

本分配范围确认一个新的静态部署边界候选 S12-01；七入口、有效标签、十组制品、三平台原生打包、纯 Server 无 Node、静态 WASM依赖、SW隔离、Release二次SHA与上传资产核验、cache容量/安全回收主链路均找到当前实现。G27已核销状态保留。候选等待总审计独立复核与去重，不宣称本轮已完成真实发行或线上验收。
