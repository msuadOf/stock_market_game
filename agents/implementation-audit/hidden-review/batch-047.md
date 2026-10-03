# 批次 047 全文复核

基线：`43b1aa5`，读取根目录 `.worktree/implementation-reaudit`。来源取主仓对应路径；只读取审计文档及基线代码/总账，未改产品文件、未运行测试/构建/Git 命令。

## EOF 与来源核验

| 来源 | 计划/实读行数 | SHA-256 | 全文章节（逐个核验） | EOF 证明 |
|---|---:|---|---|---|
| `agents/oop-refactor-audit/exhaustive/modules/tooling-01.md` | 37/37 | `541e792ab362b6f51437cc3a5dcb078bae2e9a9654db752896a41eab50599edf` | 边界依据；逐文件核销；跨文件关系和迁移顺序 | `wc -l` 为 37；连续 `cat` 从首行读至文件最后“没有运行任何测试或构建……”结语。 |
| `agents/oop-refactor-audit/exhaustive/modules/tooling-02.md` | 59/59 | `fb77ce6d0f4b2288d728e1a091ce9ff39831db2531186b10a9ff20633cd4b2db` | 现有对象聚合与总体结论；文件核销与具体设计；调用关系与不可漂移的契约；迁移步骤与验证建议 | `wc -l` 为 59；连续 `cat` 至“按本任务限制，本次没有运行测试或构建。”。 |
| `agents/oop-refactor-audit/exhaustive/modules/tooling-04.md` | 41/41 | `208b50cbf56e48cb6f305fc4e2a0bfecbd93b9fcbb609226ef654fcbd771262a` | `baseline-run.mjs`；`baseline-run.test.mjs`；`escrow-performance-harness.mjs`；文件关系与范围 | `wc -l` 为 41；连续 `cat` 至“不能把OOP对象本身当产品新能力……”末句。 |

三文件在一次连续读取中均显示完整正文；以上 hash 与 scan-plan 精确相符。记录中的较旧指令仅作为历史证据，没有沿用为现行规则。

## 章节与结论矩阵

| 来源章节 | 逐章主张核验 | 基线核验及反证 |
|---|---|---|
| tooling-01「边界依据」 | 工具脚本是 I/O 外壳，engine 与交易规则不因重构改变；指出 ADR-0017/0019/0027 边界。 | 本批无交易规则实现。基线构建保留 Server 无 Node 计划、engine fixture 显式交易所/类别/单位；无理由将一次性工具升级为领域对象。 |
| tooling-01「逐文件核销」 | 核销 clean、examples、escrow harness 及 build-targets；仅 runtime/build 提出聚合 ownership 的设计候选；同时把 probe 与进程树行为缺陷独立记录。 | 基线 build 已存在 `BuildArtifactPublisher`、`BuildRun`；escrow `runtime.rs` 是否完成当时提出的 WorkspacePathPolicy 候选没有由本批允许的 baseline 范围确认，须按审计原文记未核实。无论是否抽取，该候选是结构建议，不可误报为产品能力缺失。 |
| tooling-01「跨文件关系和迁移顺序」 | build 路径区分目录创建者与借用 worker；进程树不确定时不能假定终止；probe 写入失败与对象提取分开。 | 现行 BuildRun `build-targets.mjs:219-256` 标明 `borrowed` 并 finally 清理；监督路径 `:343-361` 有 300000ms deadline、预留清理时间和失败提示，但也直接 `process.exit(1)`，不能据此声称硬期限触发时 cleanup 已确认。G62 是整个嵌套进程树的独立总账项。 |
| tooling-02「现有对象聚合与总体结论」 | 多数 CLI/纯函数/测试维持函数形态；`CdpClient` 现有 socket/pending Map 已表达资源所有权；行为缺陷不等于 OOP action。 | 与最小惊讶原则一致。该来源是审计意见，不是实施记录；现行性能入口需单独看 G61 和后续总账。 |
| tooling-02「文件核销与具体设计」 | 对 30 个工具/测试文件逐项给 retain；登记静态包非原子失败、CDP pending、进程清理等候选 defect leads。 | 本轮未逐一重读所有 30 个源文件，不能把该章节当成源码 EOF 复核；本任务核验其审计文本及能与此批候选对应的现行 caller。缺陷线索不是新增 OOP action，也不等同已授权行为修复。 |
| tooling-02「调用关系与不可漂移的契约」 | 指明构建/发布期限、静态隔离、浏览器及进程生命周期；明确静态打包的非原子现状。 | 与大 A 无直接规则关系；不得把 Pages 静态资源处理或 CDP 观察指标映射为撮合语义。与 G61/G62 仅在异步资源终止边界上相邻。 |
| tooling-02「迁移步骤与验证建议」 | 明示不建议 OOP 重构；提出未来 bug 修复测试，但非本轮已验证结果。 | 本任务未运行相关测试。没有把“建议补测”描述为已有验证，也没有将候选补丁写入基线。 |
| tooling-04 `baseline-run.mjs` | A01 唯一新聚合候选为一次采集批次共用 deadline/permit/pool/source/fixture/resource policy；A02 证据 I/O 拆模块只是可选组织建议。 | 基线已经实现 `SimulationBatchContext`：`baseline-run.mjs:1291-1376` 聚合共享资源、矩阵执行与 finalizer；真实入口 `:1378-1394`（after）和 `:1397-1420`（sensitivity）构造并调用它。候选已落实，不能继续登记为缺失。对象不进入持久证据。 |
| tooling-04 `baseline-run.test.mjs` | 测试用 fake/临时目录/barrier 是测试资源组织，不应造生产类；该来源列出应有验证。 | 基线测试有真实 caller wrapper `baseline-run.test.mjs:60-67`；但本任务未运行测试，未从全文读出测试源码。不能声称测试通过或按名字推断全覆盖。 |
| tooling-04 `escrow-performance-harness.mjs` | A03 是 sampler reject 时不终止 child 的既有缺陷；建议最小注入 seam/清理流程，不需要创建性能服务类。 | 基线已经有 `ProcessSampleRun` 实例承载单次样本（`:325-416`），facade `runProcessSample` `:418-420` 被 `PerformanceComparisonRun.#measureSide` 默认调用 `:492-504`，再由 `runPerformanceHarness` `:596-605` 进入。`start` 在 `:391` 启 sampler、`:395-400` 先 await child close 才 await sampler；sampler 先 reject 时没有清理分支。此点仍成立，并列入 G61；对象已存在不表示异常收尾已闭合。 |
| tooling-04「文件关系与范围」 | 只提出批次 context；测试类不新增；性能 sampler 缺陷保持独立。 | 与基线一致。保持其审计性质，不把报告里的旧“候选设计，未实施”误读成当前未实施：context 当前已经存在。 |

## 现行总账关联、caller 与判定

- **G61**：现行总账 `implementation-audit-2026-10-02.md:129` 登记性能工具整体 deadline/异常资源收尾问题，包含 sampler 提前 reject 时未即时终止 child；解析表 `exhaustive-review/resolution.md:39` 将 sweep48 C48-2、sweep65 C65-1、luna48/64 汇入 G61。当前 caller 链已由基线源码确认：`runPerformanceHarness:596-605` → `PerformanceComparisonRun.#measureSide:492-504` → 默认 `runProcessSample:418-420` → `ProcessSampleRun.start:383-401`。缺陷仍在，未因类提取而解决。
- **G62**：`implementation-audit-2026-10-02.md:130` 与 `resolution.md:40` 登记嵌套 POSIX detached 进程组逃逸监督终止。tooling-01 中 build 子进程树收敛风险与此有关联，但不等于同一证据项已全修；当前 `BuildRun` finally 清理只证明代码尝试删除 owned dirs，不能证明后代均已退出。按现行总账保留 G62。
- **G/Q 关联**：三份旧审计文没有正式 G/Q 编号；tooling-01/02/04 的 ownership 提案不另造 G。工具 sampler 异常终止归 G61；嵌套树监督归 G62。未发现与本批对象提取直接相应的 Q；总账 Q01-Q23 主要是交易/跨层/范围待裁决，不能拿来给工具 OOP 候选背书。
- **候选判定**：tooling-01 A01/A02（runtime 路径与 artifact writer、BuildRun/Publisher）为结构候选；BuildRun/Publisher 在当前基线可确认已实现于 `build-targets.mjs:154-217,219-256`。tooling-04 A01 已由 `SimulationBatchContext` 实现。tooling-02 明确不建议 OOP。剩余 sampler cleanup、发布失败原子性、CDP pending 与树终止是行为缺陷线索，需沿 G 总账或后续批准需求处理，不能将历史候选当作“缺失 OOP”项目。
- **基线主体及反证**：源读取以指定 `43b1aa5` worktree 为产品证据；其相邻审计材料 `exhaustive-review/luna65.md` 对 writer 现状说明可作为二级导航，但未据此推导本批无关缺陷；`exhaustive-review/luna67.md:26,36,48` 明确记录性能 sampler rejection cleanup 当前残余，且原 action 明示排除该行为修复，是“对象化已完成但资源缺陷仍在”的直接反证。未能确认该 action JSON 所在目录下完整历史状态，故以 luna67 与当前源代码共同核对，不把它报告成代码修复已验收。
- **交易语义/范围**：无 A 股规则依据被新增或变更；审计中单位及交易规则陈述仅属“保持现有”边界。本批没有涉及沪深规则的实现，未做交易所外部规则核验。没有测试、构建或 Git 操作。

## 未核实点

- 未重读 tooling-01/02/04 清单所列全部生产/测试源文件；本任务只对三份指定审计文逐字全文读取，对 baseline 的 caller、相关实现及总账做定点核对。
- runtime.rs 中 tooling-01 所述 `WorkspacePathPolicy` / `CaptureArtifactWriter` 是否与候选逐项等价，以及 tooling-02 的静态打包/CDP 行为在当前 43b1aa5 的逐行现状，未列为本轮已确认事实。
- 未执行任何测试，因此所有测试覆盖均是文本中记录的既有断言描述，而非本轮运行结果。
