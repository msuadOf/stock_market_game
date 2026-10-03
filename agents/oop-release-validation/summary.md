# OOP 重构发布验收

本轮按用户要求完善 `.gitignore`、补必要回归、执行完整测试流程，并准备发布 `test-*` 预发行。工作分支为 `refactor/oop-complete`，重构基线提交为 `8cf34a1ce2d893f003e1d4c34d7c2bea170dd4cb`。不将用户原有 `AGENTS.md` 修改、旧 `agents/oop-refactor-audit/` 或本地 worktree 纳入提交。

## 改动与行为

`.gitignore` 注释统一为中文，补充根目录 `.worktree/`、`.worktrees/`；将用户已有 Agent JSON 规则完善为 `/agents/**/*.json`，保留 Markdown 文档和产品 JSON 的正常跟踪。

前端新增五个宿主与存档回归，覆盖失败后重试、baseline 同步期间换宿主、初始化异步操作期间 cleanup、启动档恢复失败与 stop 异常后的资源释放。独立复核发现的调用顺序断言假阳性已补为明确的调用次数断言。

完整检查发现 Rust lint/format 问题：移动 BookState 测试 module、移除无效 doc comment、冗余借用和 Copy clone、应用 rustfmt；CapturedExperienceObservation 改为直接保存最终消费者需要的 Arc，观察状态仍先独立复制和更新，不改变权威提交、错误顺序、交易与存档语义。

完整 Rust 回归发现符号价格 fixture 把入队先后误当成受理先后。修正为先验证 Sell 实际受理，再提交 Highest；保留成交、固定挂单价、序列化与恢复断言，新增相反受理顺序的确定性负控制。未修改生产撮合排序或价格笼子。

真实浏览器验收发现 CompanyPanel 在自然日触发报告刷新时，把 loading 的暂空列表当成永久空结果，丢失已选报告。新增红绿回归，改为只在 ready/empty 时协调选择，保持公开数据边界；E2E 保留原财务内容断言并强化选中状态。

发布脚本新增公开 draft 前的 tag SHA 再核验，上传期间标签变化或 API 查询失败时保留 draft。红灯证明旧实现错误公开，绿灯覆盖新门禁。两个 GitHub 请求不是原子锁，不夸称消除了所有并发窗口。

## 验证与证据

正式完整回归按现有 runner 执行两个独立五分钟进程外期限：build 密封源码和 74 个 Rust 测试二进制，execute 运行 Rust binaries、必跑跨年长例、doctests 与全部 Web 普通测试。机器可用 CPU 为 128；Rust 最多八个并行 binary，各 12 harness / 4 Rayon threads；长例 1 harness / 127 Rayon；Web 八分片。生产 WASM、Clippy 与纯 Server 构建使用 32 jobs，浏览器使用两个 workers。

本机旧 Corepack 无法加载固定 pnpm 11.19.0，使用缓存中同版本 pnpm 的正常入口继续验证，未改变版本或依赖。沙箱内 spawnSync、loopback 与 child pipe 异常均保留失败日志；在正常进程环境下复测，不把环境失败写成产品通过。

scripts 全部 32 文件最终运行通过，共 370 case，每文件和 case 均有 10000ms 门禁；聚合入口另由 300000ms 进程外 supervisor 覆盖。该长验收批次耗时 13.675 秒，最长单文件 7.961 秒，不冒称整批十秒内。详见 [逐文件结果](scripts-supervised-results.md)。Rust 格式、Clippy、生成 TypeScript 契约、生产 WASM/前端构建与纯 Server release 构建及部署 smoke 已通过。Web lint 保留三个既有 no-children-prop warning，命令退出成功，不称零 warning。

最终源码完整回归通过：Rust binaries、必跑长例与 doctests 共 2193 项通过，Web 123 文件共 608 项通过；build 6.874 秒、execute 40.876 秒，日志为 `full-verified.log`。未额外执行的 ignored 用例不纳入通过数。最终强制新构建的浏览器验收使用 CI=1、独立端口 4189、两个 workers、零重试，12/12 通过，耗时 40.8 秒，日志为 `e2e-fixed-fresh-final.log`。中间一次浏览器重跑复用了排查 preview，不计为修复后验收；随后 TypeScript 编译错误已修正，重新通过 tsc -b 和 fresh E2E。原截图证据已恢复，新截图仅保留在本轮 `.tmp` 目录。

原始 `.log`、JSON inventory 和浏览器 trace 是本地机器证据，按 `.gitignore` 不纳入提交；本文与其他 Markdown 中的日志路径供本工作区复核，不保证 GitHub 上存在对应原始文件。正式远端发布另以 Actions 日志和 Release provenance 核验。完整流程边界见 [流程检查](release-process-checklist.md)，发布门禁红绿见 [TDD 记录](publish-tag-sha-tdd.md)，独立复核见 [review.md](review.md)。

## 发布与适用边界

第一轮发布提交为 `0e64ae7458ae1ba0fffa05de58f02faf443e5536`，标签 `test-20261003-051652`，Actions run 为 `37099361368`。Ubuntu 全部 CI 通过；Windows 的完整回归通过后，依赖隔离测试中的 cargo tree 查询触及五秒子进程上限。没有创建公开 Release。按原断言和时限启动的第二次 Windows 定向重跑，已依用户随后要求取消。

用户最新决定是去掉 CI 检查，仅保留 build release。发布配置据此调整为：tag 与 SHA 校验 → 三平台 Desktop/Server/WebUI Server 及静态 Web 构建打包 → 十组 manifest 与远端资产完整性校验 → prerelease → Pages。发布链路不再调用 CI、测试套件或 smoke；CI 保留为独立手动开发诊断入口。先前本地完整验收结果继续作为实际执行证据，不宣称新的发布流水线仍有双平台测试门禁。旧标签保留且未移动。

新的 build-only 发布提交为 `d97ac3937f6507a87a49ef00cf2f640206df6337`，标签为 [`test-20261003-054925`](https://github.com/msuadOf/stock_market_game/releases/tag/test-20261003-054925)，对应 [Actions run 37101092414](https://github.com/msuadOf/stock_market_game/actions/runs/37101092414)。三平台 Desktop、Server、WebUI Server 以及共享前端构建全部成功，Release 已公开，`prerelease=true`、`draft=false`。独立下载十组 manifest 和 `release-source.json`，确认来源 SHA 与发布提交一致，35 个资产的名称集合、大小及远端 SHA-256 均与 manifest 一致，下载的 JSON 文件自身摘要也一致。Release 说明已改为中文。

Pages deploy 已成功，直接复用本轮站点 artifact，因此 `pages / build` 按设计跳过；分发内部缓存清理由发布末尾统一处理，其内部清理 job 同样按设计跳过。末尾 `prune-caches` 成功，整个 Actions run 最终为 `completed / success`。本次没有运行线上 smoke，不把部署成功等同于公网游戏完整验收。

未扩展既有 ignored scale/stress/cost 验收、完整 K7 矩阵、原生 GUI 安装、签名或公证；不宣称这些额外范围通过。现行 Rust runner 的普通 case 没有独立十秒 watchdog，本轮遵循原有完整回归长验收入口，不能以批次通过证明每 case 十秒门禁。A 股规则、金额分/股单位、T+1、实际受理顺序和日终存档约束不变。
