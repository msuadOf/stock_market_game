# 批次 121：Tooling 与元数据标签记录复核

## 范围与读取确认

依照连续全文读取至 EOF 的要求，逐篇读取了三份历史报告。哈希与行数复核结果：

| 来源 | 行数 | SHA-256 | 读取状态 |
|---|---:|---|---|
| `agents/oop-refactor-audit/chinese-localization/reviews/hosts-tooling-a.md` | 27 | `218e5ba167e7648fd70d85e92c0222fb9a234db8fde9dcda531d24091b6a09bb` | EOF |
| `agents/oop-refactor-audit/chinese-localization/reviews/hosts-tooling-b.md` | 28 | `6db1714362c21d043461c54ff7f3fb739aad43cfe63ac8c5b909a4b8aea6ff1c` | EOF |
| `agents/oop-refactor-audit/chinese-localization/reviews/metadata-labels-final.md` | 11 | `cf5ff15061f81823f6e358520006f2d824b099168f0f4294427be33b43e2eedd` | EOF |

章节族：前两篇为 tooling 中文化 diff 复核，含结论/范围、JSON结构与语义核对、冻结 SHA 表；第三篇为 metadata 标签中文化 diff 复核，含范围声明与大A语义、最小范围、跨层边界三项结论。前两篇仅证明当时的本地化差异未改变所列内容，不等同源码或交易规则审计；第三篇也明确不复核源码和历史结论。旧报告自身“通过”结论不作为当前代码证据。

## 43b1aa5 / 后续总账核对

`43b1aa5` 是 2026-10-03 的历史需求穷尽复核文档提交，涵盖当时全量需求源扫描、调用链核实和候选裁定；它不是当前产品行为的自动证明。该提交及后续 `implementation-audit-2026-10-02.md`、`coverage-index.md` 明确区分已实现、缺口、待决和被后续 ADR 取代项，并规定 OOP 提取不能核销行为缺口。此次范围是两篇 tooling 文档复核和一篇 metadata 文档复核，不承接所有 G01–G68/Q 条目的重审；三篇本身未提出 G 编号或交易规则主张。与 tooling 有交叉的当前总账事项（例如 G26 的 Web lint 边界、G39 的自由调度 artifact 相等契约）应按总账各自的新鲜 caller/ADR 证据处理，不能因本批旧报告的“通过”而核销，也没有证据在本批范围内反证它们。

后续决策核查以当前树为准：ADR-0027/0028 对 server 构建、发布和静态 Pages 的职责/范围约束优先于早期 OOP 文案；并没有发现明确决策批准静默改变进程清理、符号链接 containment、CDP 断连结算或自由调度确定性契约。旧 tooling 记录的缺陷线索必须重新看当前实现，不能只凭旧记录认定仍存在。

## 当前实现对照与裁定

- 旧 tooling OOP 候选已不能整体称作“未实施”：`scripts/build-targets.mjs:154-216,219-351` 当前已有 `BuildArtifactPublisher` 与 `BuildRun`，因此旧候选中的构建目录/发布职责提取至少部分落地；这只证明代码结构，不证明所有者清理或失败/超时行为正确。无需因旧对象候选再提出同一结构改造。
- `scripts/smoke-pages.mjs:12-25` 当前将请求路径 `path.resolve` 后直接 `readFile`；旧 tooling 区域记录的词法 containment 可被 root 内 symlink 指向外部内容这一边界仍有反证支持。当前扫描未发现此处 realpath containment。属于安全边界候选，不能归入 OOP 重构或视为已修复。
- `scripts/run-with-deadline.mjs:13-22` 当前仍分别通过 Windows `taskkill /T /F` 和 POSIX 负 PID `SIGKILL` 请求终止；调用后是否等到完整后代树退出，不能从发出信号推断。此为进程树确认边界候选，需与共享 deadline/清理规则一起裁决，不将它夸大成所有 deadline 失效。
- Tooling 区域的旧记录提出 CDP pending promise、采样失败 child 清理、静态打包失败处理以及若干路径/产物写入线索。它们不在三篇旧中文化报告的源码复核范围内；本次未以旧报告或本批全文读取声称逐一重新验证，候选状态保留为“待主审按当前 caller/实现复核”，而非已解决或确定仍存在。
- `runtime.rs:1099-1146` 的 workspace 路径测试只展示了相应测试源码存在；此任务未运行测试。其对 workspace path validation 的覆盖不可外推至 Node 静态 Pages 路由。
- metadata 标签报告只涉及 review 文档用语、路径、摘要和 key 的保留，不含生产调用链，不构成交易语义变化或产品实现候选。没有从其历史标签/措辞差异推出代码缺陷。

## 结论

三篇旧报告的范围和限制陈述可信，但其结论仅对当时文档 diff 有效。当前候选中，构建 owner 提取已部分反映于现行实现，结构候选应据此收窄/不重复；Pages 符号链接 containment 与 deadline 进程树退出确认仍有具体源码边界，作为独立行为候选提交总审计复核。其他旧 defect lead 因本批范围不足，不宣称复证或核销。未发现新增 A 股语义判断；没有产品改动，也没有运行测试、构建或 Git 写操作。
