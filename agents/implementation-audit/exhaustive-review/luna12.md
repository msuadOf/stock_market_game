# Luna 12：发布、构建部署与 Actions cache 独立复核

## 基线与读取范围

- 目标 worktree：`/data1/baiyifan/workplace/stock_market_game/.worktree/implementation-reaudit`。
- 产品基线：`08e4fc7`；当前 HEAD：`a7c7ce357bdc9f88c03633744b2d5815db49e9b2`，是审计 merge，相关产品代码与产品基线相同。
- 连续从首行读至 EOF：`docs/decisions/0028-tagged-release-and-static-pages.md` **103 行**、`docs/build-and-deployment.md` **335 行**、`docs/actions-cache.md` **60 行**，合计 **498 行**。完整章节矩阵见下文。
- 读取 `AGENTS.md`、`docs/principles.md`，并追踪当前 workflows、发布脚本、缓存脚本、Pages base 构造与关联测试源码。未运行测试、构建或长验收；未调用线上 API、部署或执行 Git 写操作。只写本审查记录。
- 当前 ADR-0028 明定 2026-10-03 起 Release 为 **build-only**。CI 仍是独立手动开发诊断入口；不将被取消的 Release 测试、lint、Clippy、浏览器 smoke 门禁作为遗漏。

## 文档全文章节矩阵

| 文档章节 / 原文行号 | 生产 caller 与实现核对 | 结论 |
|---|---|---|
| ADR-0028 状态、决策 `:1–35` | `.github/workflows/release.yml:4–5` 只接收 `v*`/`test-*` 标签；`:26–30` 调 `release-policy.mjs` 校验并冻结 SHA；`scripts/release-policy.mjs:4–18` 做严格语法校验。`:32–35` 进入可复用 distributions workflow；七个独立 build workflow 均为 `workflow_dispatch`。 | 普通 push/PR 不自动构建；有效入口与版本/预发行分类符合文档。标签过滤宽泛但实际编译前拒绝无效标签。 |
| ADR-0028 静态运行边界 `:37–55` | `distributions.yml:85–109` 构造 Pages 包并上传 Pages artifact；`apps/web/src/main.tsx` 与 Pages isolation 模块先完成隔离再导入 App；`pages-isolation.js` 限定同源静态请求。普通原生目标未启用 pages mode。 | 找到静态 WASM、子路径、SW 与独立 smoke 的对应生产路径；本轮不作线上验收声明。Pages base 分类存在既有候选 S12-01，详见下文。 |
| ADR-0028 缓存、权限与验收 `:57–93` | `release.yml:38–42` 发布 job 与权限分开；`:65–74` Pages reusable caller；`:76–103` 统一 cache-prune。`distributions.yml:38–39,145–146,265–266` 只读编译叶 job；`:114–139` 清理叶 job才有 `actions: write`。`scripts/prune-actions-cache.mjs:24–68,84–160` 校验 ref、活动 workflow、成功 Release、删除计划及剩余容量。Cargo 路径仅白名单目录。 | build-only、权限边界、成功后退休 tag cache、失败保留、活动任务暂缓和 alias 检查均有当前 caller。文档所说的五分钟边界与工作流调用一致。 |
| ADR-0028 技术依据 `:95–103` | 当前 workflow 声明 Pages `queue: max` 与独立部署组；本轮仅核现存契约，没有联网重查官方文档。 | 这是决策依据/链接章节，不构成缺失的产品入口。 |
| 构建与部署引言、构建目标 `:1–59` | `scripts/frontend-build.mjs`/`frontend-build-plan.mjs` 承担锁定工具、WASM、发布边界、TypeScript/Vite 与 deadline；`scripts/build.sh`、`build.bat` 分发四个目标至 `build-targets.mjs`；目标规划拒绝覆盖已有制品。 | 统一入口、依赖方向、并行预算和输出目录契约能追到当前实现。 |
| 构建机与部署机 `:60–90` | Server shell/batch 入口在 Node 前分流；`build-targets.mjs` 获取 rustc host target、使用隔离 Cargo target/cache 并清理 bundle；Desktop 使用固定 CLI，未签名选项在打包调用中。 | 构建依赖与部署依赖区分明确；文档没有承诺无操作系统运行库。 |
| GitHub 三平台未签名分发 `:91–183` | 七个入口 workflow 调用 `distributions.yml`；`:20–35` 事件/产品输入；`:38–114` shared frontend、Pages artifact 和清理；`:145–266` native/server matrix；`package-distributions.mjs` 检查 promised formats、文件非空、架构、链接及目标路径。 | 七按钮、artifact 传递、三平台原生构建与打包边界有当前生产调用链。未经签名、安装器启动与平台运行库验收被原文明示为范围限制。 |
| 静态 Web 与 GitHub Pages `:185–214` | `distributions.yml:85–98` 生成静态包并以当前 `github.sha` 写 build-info；`:107–112` 上传同 run Pages artifact；`build-web.yml:20–47` 手动构建/部署，`release.yml:65–74` 发布后复用同 run site，`build-all.yml:18–29` 全量成功后部署。`PAGES_BASE` 当前计算见 `distributions.yml:85–93`。 | Pages 主链路、手动/标签/All 入口均有 caller。仓库名与 owner 的根站点判定仍有候选 S12-01。 |
| 服务启动三方式 `:216–244` | `apps/server/src/deployment.rs` 解析 services/bind/web-root；静态资源、API 与未知路径分别路由。 | 文档模式能映射当前服务端实现；本次不重做 Server 专项审计。 |
| 启动时选择引擎 `:245–256` | UI host lifecycle 选择 Desktop/WASM/RemoteHost；构建脚本过滤生产 host/token 环境变量。 | 文档描述与当前生产入口相符；此项不替代宿主协议审查。 |
| 协议与浏览器约束 `:258–271` | Server 静态响应隔离头和 UI startup policy 均有当前实现；HTTPS 代理、证书及远程 CORS 是部署约束。 | 记录的是能力与外部约束，没有发现被宣称已实现的额外代理/认证功能缺口。 |
| 验证范围及 2026-10-02 七入口、标签、线上记录 `:273–335` | 文档按 run、SHA、artifact、浏览器 fixture 和限制记载历史结果；`:333–335` 明确旧日期标签 CI 与 2026-10-03 build-only 变化。当前源码 build-only caller 见 `release.yml:32–74`。 | 历史验收证据不是本轮新实测。没有把旧 CI 结果冒充当前 Release 门禁，也没有发现文档宣称本轮验证通过。 |
| Actions cache 目标与手动命令 `:1–22` | `prune-actions-cache.mjs:24–68` 按 ref/key series 保留滚动快照；`:79–101` 默认 dry-run/参数约束；`:132–159` 活跃 workflow、逐项删除与预算核验。 | 手动命令、超时、未知 key 保护、失败显式返回均有实现。 |
| Actions cache 自动清理 `:24–46` | `ci.yml:20–48`、`distributions.yml:114–139`、`release.yml:76–103` 使用独立 cleanup job；各自串行组与权限受限。标签分发清理跳过，Release 收尾负责退休；CLI 再查已公开 Release 以补做先前延期退休。 | 生产调用链符合新 build-only 与收尾策略；没有发现需要新增持久化待办状态的依据。 |
| Actions cache 历史清理与测试命令 `:48–60` | 该节是 2026-10-02 的历史数字及既有短测命令；本轮仅查看实现，没有执行该测试命令。 | 历史事实按原文记录；不报告为当前 API 数值或本轮测试结果。 |

## 旧结论复核

- **G27 已修，保持核销。** `scripts/publish-release.mjs:83–84` 在构建资产前核 tag SHA；`:86` 创建 draft；`:93–107` 核远端 asset inventory、名称、大小与 digest；`:109–113` 在公开前再次读取 tag SHA，只有匹配才移除 draft。标签不匹配/查询异常不会公开。两次 API 请求不是原子标签锁，现有要求与文档也未宣称原子性；不重开 G27。
- **旧 sweep12 的 S12-01 仍成立，但不是本轮新发现。** `agents/implementation-audit/exhaustive-review/sweep12.md` 已记录：`.github/workflows/distributions.yml:88–89` 只传 `github.event.repository.name`，凡仓库名以 `.github.io` 结尾就使用 `/`。根站点资格实际还要求仓库名与 owner 对应；例如 owner `alice`、项目仓库 `docs.github.io` 会被错判为根站点，项目所需 base 应是 `/docs.github.io/`。该构造用于 `product == web || all`，因此影响手动 Web、All 与标签发行静态包。当前仓库名 `stock_market_game` 的既有线上路径 `/stock_market_game/` 不受影响，不能推翻历史线上验收。建议维持为待总审计去重的 Pages base 分类候选，不重复编号或称为本轮新增。
- **build-only 是当前决定。** `release.yml` 的 DAG 是 validate → distributions → publish → Pages → cleanup；没有 CI reusable workflow 或测试/lint/smoke job。`ci.yml:6–7` 仍只接受 `workflow_dispatch`。生产 TypeScript/WASM 和 manifest 检查在 `distributions.yml` 中保留，符合“只构建”决策，不是测试门禁回流。
- **权限与 cache 旧疑点已排除。** reusable workflow caller 在 `distributions.yml` 授予 `actions: write` 供其 cleanup job 使用，但 frontend/native/server 叶 job 自身声明只读权限；清理步骤位于独立 job。Release cache 清理在 publish 与 Pages 之后统一运行；标签任务失败时走普通保留策略，不传 `--retire-tag`。
- **整 `target/` 旧缓存风险已排除。** 当前 WASM Rust cache 使用 `cache-targets: false` 和显式编译目录；native cache 只回存 registry/git 与 `target/build-cache/<product>`；最终分发通过当前 run artifacts，不从 target cache 恢复。

## 候选与反证

| 候选/疑点 | 复核结论与证据 |
|---|---|
| S12-01 Pages 项目仓库误识别为根站点 | 保持候选；实现只看 repo name 后缀，缺 owner 比较。真实 GitHub Pages 语义的项目仓库名可与 `<owner>.github.io` 不同，故反例可成立。当前仓库路径通过只排除当前仓库受影响，不排除通用入口边界缺口。 |
| Release 未跑 CI / 测试 / lint / Clippy / smoke | 排除为实现遗漏；与 ADR-0028 `:15–18`、build 文档 `:143–147` 的最新 build-only 决定一致。 |
| 上传资产后 tag 可移动 | G27 已修；`:109–113` 二次 SHA 守卫并在其后才公开。仅保留 API 请求非原子这一诚实限制。 |
| 七入口、静态 artifact、标签发布 SHA 是否漂移 | 手动 workflow 使用事件的 `github.sha`；native 下载 `production-frontend-${{ github.sha }}`；静态 build-info 使用当前 SHA；Release collector 绑定 validate 输出 SHA。没有执行中重读 branch HEAD 的 caller。 |
| 发布前十组 manifest / 压缩包缺少完整性核验 | publisher 收集十组目录并核本地/远端名称、大小、SHA-256；package producer 检查平台要求格式。未发现 build-only 删除这些完整性门禁。 |
| cleanup 延期后没有回收记忆 | cleanup 每次扫描已公开且含 `release-source.json` 的 Release 来推导 retired tags；ADR 明确不要求另存回收状态。 |
| Root Pages SW 缓存/API 拦截 | SW 仅限定 scope、同源 GET 与静态资源列表，原请求交给 fetch 并保留响应主体/状态/类型；未见 CacheStorage 或 API 请求白名单。既有候选不涉及此处。 |

## 收口

本范围没有发现除已记录 S12-01 外的新独立遗漏。G27 保持已修复/核销；Release build-only 与手动 CI 分离符合最新决定。S12-01 在本 worktree 现状中仍可由 `distributions.yml:88–89` 直接复现，应交由总审计与其他分片去重、定级。本报告未执行构建、测试、线上检查或交易语义变更；发布历史与 Pages 线上状态仅按文档证据引用。
