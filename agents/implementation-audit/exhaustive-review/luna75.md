# OOP 实施与发布材料全文复核

复核对象：合并提交 `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`（第二父提交 `08e4fc75b52a71a3262a8a938c57b44f8b5b4960`，提交主题为 merge `08e4fc7`），仅审阅文档、验证索引、其引用的台账/当前源、以及 build-only 工作流。未运行产品测试、构建或长测；未改产品文件及 Git 状态。

## 行数与章节矩阵

| 材料 | 行数 | 章节/范围 | 本轮复核 |
|---|---:|---|---|
| `agents/oop-refactor-implementation/summary.md` | 44 | 实施入口、行为边界、实际验证、原有工作范围 | 核实 128 action 分类入口、边界承诺、短测/未覆盖声明与交付范围；回到各 ledger 和验证 JSON 对照，未把摘要的结论当当前重跑结果。 |
| `agents/oop-refactor-implementation/validation/rust-short-plan.md` | 600 | 执行参数、编译记录、21 target 分组、范围限制、R001–R278 全部 case、128 action 映射、版本绑定 | 扫描全部精确 case 行和 mapping 行；以 `rust-short-plan.json` 的 key、proof、source hash、action coverage 为索引，与 128 action ledger、生产 caller 字段和对应 source 声明核对。逐项对应仍以机器索引完整 key 为准；报告不把 278 case 理解成 278 个 action 独立验收。 |
| `agents/oop-release-validation/build-only-change.md` | 41 | 发布需求/变更说明、验证及限制 | 对照当前 `release.yml`、`distributions.yml`、`ci.yml`、`build-web.yml` 和后续 release validation 记录，区分短契约、workflow 结构校验、实际构建和发布事实。 |

行数为本次读取时 `wc -l` 结果。短测索引 target 章节行 70–452，逐项覆盖 R001–R278；action 表行 453–586，逐项列出 128 action；最后版本绑定章节行 588–600。

## 逐项追溯结论

- **Case → proof**：索引按 package/target 分组列精确 `--exact` 过滤名、proof record、测试声明行；JSON 保存完整语义 key、`passed`、`exit_code`、超时状态、来源 SHA 与 final proof 引用。文档明示 278 唯一 case = 273 常规 + 5 Writer；JSON 的 `cases:273` 是常规数、`already_executed_cases/catalog_cases:278` 为总数，字段间不是总数矛盾。
- **Case → action**：索引 R001–R278 均有源/目标记录，action 表标出 Rust case IDs、Web/Node 承接或未运行长周期 fixture；`rust-short-plan.json.action_coverage` 给出机器映射与 case 状态。共享 case 服务多个耦合 action，文档明确“不声称一个 action 必须新增独立测试”。这是一种承接关系，不是每个 action 独立行为穷举。
- **Action → 实际 caller**：128 action 的实施台账（domain、pipeline、session、hosts）存放实现位置、生产 caller、测试 caller/证据；例如 `session/action-ledger.json` 的 `production_callers` 和 `additional_caller_review` 字段、`pipeline/implementation-ledger.json` 等记录是实际调用链证据。短测索引本身只给测试声明，不足以单独证明生产可达性；调用关系应按相应 ledger 和 caller review 阅读，不能把 case 名称视为 caller 证明。
- **Case → 后续验收**：`summary.md` 第 28–37 行与短测索引第 57–66 行均写明本轮未运行完整回归、浏览器 E2E、长期模拟/性能矩阵，N04 与 R2-N05 长周期 fixture 只编译并完整 diff 复核，`compile_fail` doctest 也未运行。final proof 可证明所记 source SHA 对应的历史精确 case 结果，不覆盖未执行项目。
- **build-only → 后续发布**：变更记录描述删除发布路径的 CI/smoke 契约和依赖，同时保留生产编译、分发产物、manifest 校验、Release/Pages 发布与安全缓存清理。当前 `release.yml` 确实先验证标签/SHA，再调用 `distributions.yml`，再发布并部署 Pages；`ci.yml` 当前只有 `workflow_dispatch`，内部仍含手动开发诊断测试；`build-web.yml` 仍通过 reusable `distributions.yml` 构建/部署 Pages。故“去掉 CI 检查”限定为标签发布链路，不能解读成仓库所有 workflow 不再有测试。

## 原结论复核与候选反证

1. `summary.md` 声称“278 个唯一 Rust 精确 case 全部通过”，索引与 JSON 的数量、proof 数量吻合；`final-validation.json` 将 `case_and_command_deadline_ms` 记为 10000，最大 case 1.778s，命令均有退出码。历史 run 的通过状态有 SHA/record 绑定。但当前 HEAD 是 merge commit `a7c7ce3…`，而证明是既有 run 记录；当前没有重新执行，也不应把它们表述为当前 HEAD 的全绿结果。短测索引末章也明确维护者本次只更新元数据，没有重跑 Cargo/test binary/`--list`。
2. “128 action 完成”由正文链接的五份实现台账支撑；检查重点是账本中的 owner/method/caller 和覆盖声明，而非计划状态标签。短测映射包括共享 case、27 个非 Rust 承接项及两个未运行旧长周期项；就证据范围而言没有发现把未运行长周期 fixture 伪装成运行通过的说法。风险是摘要第一段“落实全部 128 个动作组”容易脱离同页限制被单独转述为全行为验收；应连同本报告列出的未覆盖边界读取。
3. 大 A 语义声明限定为重构保持金额分、股数、T+1、累计费用、局部受理、优先级和失败写入边界；本批不变更交易规则，也未给出规则更新依据，因此不能将本次短测说成对现行交易所规则的重新验证。对应 test name 和语义保护点使用 A 股术语，但无证据显示本批引入交易制度变更。
4. build-only 旧验证记录称当前契约 20/20、最终相关短测 59/59，PyYAML 检查仅称 YAML/job-needs 引用解析，明确 actionlint、三平台构建、实际发布/线上 smoke 未由该批次执行。后续 `agents/oop-release-validation/summary.md` 记录了 tag 对应真实三平台构建和发布以及下载资产核对；该后续记录是独立发布证据，不能倒灌为 build-only-change.md 这批命令做过线上构建。当前 workflow 配置与 build-only 描述相符。

### 原文锚点（逐字）

- `summary.md:30`：“**278 个唯一 Rust 精确 case 全部通过**”；`summary.md:35`：“本轮按用户要求**没有运行复杂回归、浏览器 E2E、长期模拟或性能矩阵**”。同一摘要相邻说明限定证据边界。
- `rust-short-plan.md:590`：“本次仅做 JSON 整体解析、source SHA/声明行更新与两项迁移元数据校正，没有重跑编译或测试。”
- `rust-short-plan.md:591`：“……不将机械核对称为重新阅读全部源码或独立完整 diff 审查。”说明 index 更新动作本身不构成重新验证。
- `rust-short-plan.md:453`：“映射到 Rust case 的 99 个 action，其列出的 case 已全部通过；27 个 action 由 Web/Node 证据承接；两个旧长周期 fixture 只承接编译与完整 diff 复核。”后两者不可折算成 Rust case 通过。
- `build-only-change.md:12`：“`ci.yml` 移除 `workflow_call`，保留 `workflow_dispatch` 及原有开发诊断测试。”与当前 `ci.yml` 手动触发及测试 job 相符。
- `build-only-change.md:35`：“这是 YAML 语法和引用检查，不称为 actionlint 全绿；本轮没有运行 actionlint。”且第 36 行明确未运行三平台实际构建、发布或线上 smoke。

## 新发现 / 复核建议

- 本轮未找到足以推翻既有 case 的确定性代码反例；旧的 pass 结论仅对绑定的源码快照和精确命令有效，不升级为当前 HEAD 的验证结论。
- 发布策略当前仍通过 `workflow_call` 调用 distributions 与 build-web。这是预期的复用构建路径；“去掉 CI 检查”只表示 release workflow 不依赖 CI job，且 distribution 内保留产物构建/校验，不表示发布 workflow 不再执行任何检查。
- 当前未运行产品测试、构建、官方交易规则核验或发布动作；本报告仅为文档、台账、声明行、proof 元数据和 workflow 的独立静态复核。没有交易语义变化可供本轮另作制度合规背书。
