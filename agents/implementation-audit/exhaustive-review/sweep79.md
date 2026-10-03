# sweep79：scripts 三轮结果、监督与发现范围

## 全文与原始对账

- 连续全文读取 `agents/oop-release-validation/scripts-results.md`（40 行）、`scripts-supervised-results.md`（40 行）、`scripts-unsandboxed-results.md`（54 行），共 134 行；每张32文件结果表完整读取。
- 产品基线为 `b76ece3` 合并同等代码，只读审计，没有产品改动、Git 写或测试运行。本批未确认新 G；正式 scripts 完整发现/持续覆盖入口仍是原 Q05，不能重写成完全没有发现代码。
- JSON/日志在审计 worktree 没有复制，故只读原工作树 `/data1/baiyifan/workplace/stock_market_game/agents/oop-release-validation/` 对应三个 JSON 及诊断日志。首次读取 worktree 的 JSON 返回 ENOENT 后改查原树；没有生成、补写或覆盖原结果。
- 三个原 JSON 均为 Node v25.8.2、available_cpu_count=128、file_count=32、4 worker、case_timeout=10000、external_file_deadline=10000、test-isolation=none、文件内test-concurrency=1。readonly递归扫描当前 `scripts/**/*.test.mjs` 也为32文件；数量相等不证明源码/内容相同或当前已通过。

| 全文记录与原文行号 | 原JSON静态实算 | 当前可得结论 |
|---|---|---|
| `scripts-results.md:3` 环境/资源，`:5`首轮，`:7`全部32行结果 | 32条，27 exit0；五失败为desktop build-matrix/full-regression/deadline/smoke/wasm dependency；wall8357ms。 | 历史首轮原样保留，不用后续green覆盖原失败 |
| `scripts-supervised-results.md:3`资源，`:5`完整长验收，`:7`32文件表 | 32条全部exit0、wall13675ms。 | 聚合已明确长验收，单文件仍受十秒门禁；不是完整游戏Rust/GUI/K7验收 |
| `scripts-unsandboxed-results.md:3`资源，`:5`32文件369case，`:7`首轮诊断，`:19`授权范围，`:21`结果表 | 32条全部exit0、wall7820ms；JSON文件级结果与文档一致。369case是原记录声明，本批未逐一重新解析全部TAP计数或执行case。 | 只代表当时沙箱外脚本短fixture；没有运行外部发布或改产品 |

三文档和 runner JSON 没有为每个测试源码写入commit/SHA签署字段。因此即便当前仍32文件，也不能把历史32/32或369/369当当前版本运行结果。本批没有补造来源指纹或重新署名旧运行。

## 发现、监督与错误边界的现行源码

| 原文要求/承诺 | 当前caller/实现行号 | 状态 |
|---|---|---|
| 三文档`:3`「32文件、4文件worker」 | `agents/oop-release-validation/run-scripts-tests.mjs:15` 递归readdir，`:19`收集`.test.mjs`，`:22`从scripts根发现，`:23`排序，`:50`启动4 workers；没有硬编码32文件清单。 | 自动发现已实现，4进程明确多核，不是32个文件串行 |
| 三文档`:3`每文件进程外10秒/case10秒 | runner`:33`调用真正 `scripts/run-with-deadline.mjs 10000 -- node --test --test-timeout=10000`，文件内concurrency1/isolation none/显式TAP。 | 已实现；不是只在被测Node事件循环计时 |
| `scripts-supervised-results.md:1`长验收含全部短文件/结果写入 | runner`:59`–`:64`通过 `runBoundedCommand` 启动worker，外层300000ms/cleanupReserve1000；本体等待4workers、写logs与JSON/Markdown都在外层期限内。 | 聚合13.675秒与单文件十秒不矛盾；外层没有放宽普通文件门禁 |
| 三文档结果必须依据真实exit/保留失败输出 | runner`:38` spawn stdout/stderr pipes，`:41`error记录null+error，`:42`close记录真实code/signal，`:45`输出原命令和原日志，`:52`任何exit≠0为失败，`:55`batch非零。 | 非零或启动失败不会被空输出冒充成功 |
| 超时必须终止进程树与hard cutoff/清理 | `scripts/run-with-deadline.mjs:10` Unix process group SIGKILL/Windows taskkill；`:69`实际spawn且Unix detached；`:80`执行截止杀树，`:84`total hard deadline拒绝未close，`:125`timeout显式错误；`:143`移除timer/listener。 | 监督主链存在；未实测Windows终止或所有机器调度 |
| 普通/长期限不可放宽 | deadline`:150`默认10000，`:152`拒普通deadline>10000；`:29`拒总deadline>300000；`run-long-validation.mjs:9`同样约束300000。普通短测 `run-with-deadline.test.mjs:38`never-close second cutoff、`:55`输出、`:94`callback failure、`:117`abort/close。 | 有production guards与代表性测试源码；本批未运行 |
| 完整发现的正式根/CI接线 | 根 `package.json:15`是run-full-regression，生产runner `scripts/run-full-regression.mjs:76`只将Web正式runner纳入全回归；手动CI `ci.yml:42`/`:96`/`:205`选定脚本套件，没有调用任务目录完整scripts runner。 | 原Q05仍待定；不能因为相邻选定脚本运行就宣称完整统一覆盖 |

当前任务目录runner允许每文件全部结果收集后才决定退出，正是原记录明示的诊断批次行为。它不是正式生产root regression的fail-fast资源策略，也不能以此要求给所有被测产品代码加重试或fallback。该runner没有产品网络权限或发布功能。

## 五项首轮失败的后续证据与反证

| `scripts-unsandboxed-results.md`原文行号/首轮文件 | 后续静态证据 | 处理 |
|---|---|---|
| `:13`desktop build-matrix | 原树 `scripts-sandbox-diagnostic-desktop-build-matrix.log:9`、`:29`明确spawnSync /bin/bash EPERM；unsandboxed`:28`与supervised`:14`记录exit0。 | 环境禁止实际shell，不登记产品planner未实现 |
| `:14`full-regression | 原文明确spawnSync Node EPERM/诊断输出未送达；unsandboxed`:40`与supervised`:26`成功。 | 不将空JSON/日志条件失败当实际Cargo编译坏，仍须保留首轮失败 |
| `:15`run-with-deadline | 原树诊断log`:27`、`:52`、`:77`显示stdout/stream/failure输出断言，原文未把根因过度断言为单一EPERM；unsandboxed`:44`与supervised`:30`成功。 | 不能据空输出推断无deadline实现，也不编造Node官方缺陷 |
| `:16`wasm-build-dependencies | 原树诊断log`:9`、`:29`、`:50`三个case均spawnSync cargo EPERM；unsandboxed`:54`/supervised`:40`成功。 | 禁止spawn的环境限制，不是native/WASM依赖隔离断言失效 |
| `:17`smoke-deployment | 原文明确loopback listen EPERM并有child空输出；unsandboxed`:53`/supervised`:39`成功。 | 不把无法listen误报实际Server路由不存在；短fixture通过也不证明公网部署 |

`scripts-results.md:5`仅给五失败数，不能单凭第一次空捕获识别每个原因。后续显式TAP诊断才支持上面的环境判断；两轮green也不抹掉第一次真实失败。`scripts-unsandboxed-results.md:19`的approval是该历史执行授权，本批没有重新调用外沙箱权限或运行任何测试。

## 全部32文件结果表的产品范围

三表涵盖构建目标/九格desktop dry-run、缓存工作流、文档符号、release WASM导出、CI及分发/Pages、frontend build、full regression/Web shard、发行打包/资产发布、性能报告、deadline长短监督、baseline/escrow/K7 root validator及smoke/WASM依赖。它们都是脚本或短fixture的测试记录，不能合并声称真实native三平台编译、实际GitHub发布、浏览器/Weston视觉、真实K7矩阵或完整游戏测试通过。

`check-web-release-wasm.test.mjs`通过验证对应解析/守卫；实际build `apps/web/package.json:8`才调用 `check-web-release-wasm.mjs`。`publish-release.test.mjs`通过替代gh调用覆盖行为；生产 `scripts/publish-release.mjs:109`的公开前tag核验属于已核销G27，不将32文件green重复当第二个新修复。K7脚本纯验证套件通过不能核销既有G39的现行跨worker完整产物比较问题。

## 新候选与判定

- 「没有scripts自动发现」已被递归runner反证；正式入口维护范围仍只保留Q05，不新增G。
- 「13.675秒违反普通十秒」由文档显式long aggregate与外层300000ms、每文件/case10000ms反证，不扩大普通超时。
- 「27/32说明产品缺五功能」由五套TAP实际环境诊断和后续两轮完整成功反证；第一次错误原样保留。
- 「32/32证明当前b76ece3全通过」没有逐源码commit/SHA签署，且未本轮运行，不能成立。
- 「选定CI套件等于全scripts覆盖」正式调用链不支持；Q05仍待定，最新ADR0028/`docs/testing.md:124`明确产品发布不跑tests/lint/smoke，不为补Q05恢复旧发布门禁。

本批只有readonly JSON/源码/日志核对，没有重测历史case、改变断言或生成新成功结果。未发现这三篇后续工作记录引出的新增生产漏实现。
