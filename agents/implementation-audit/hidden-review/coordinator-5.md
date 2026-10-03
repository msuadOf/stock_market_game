# Owner 5 批次协调总结

## 覆盖情况

- `scan-plan.json` 中 `owner=5` 共 45 批，ID 为 005、010、015、020、025、030、035、040、045、050、055、060、065、070、075、080、085、090、095、100、105、110、115、120、125、130、135、140、145、150、155、160、165、170、175、180、185、190、195、200、205、210、215、220、225。
- 已检查上述批次各自的 `.md` 和 `.json` 产物均存在于计划 `output_root`；批次源文档均由对应 reader 报告连续读至 EOF，哈希及行数按计划核对。批次 025 额外读取的 `session/parts/25.md` 已移到 `collateral`，其 `sources` 仅保留计划中的 parts/12–14。
- 批次 130 曾因长输出截断而需要补读；最终报告列出的三源行数和 SHA-256 与主工作树实测一致，JSON 中三项 `read_to_eof` 均为 `true`。
- 批次 145、150、165、175 曾缺少机器要求的逐源字段，已由原 reader 按其实际 EOF 阅读凭证补齐 `sources: [{path, sha256, lines, read_to_eof}]`。批次 175 的 D01 与主控已登记的 G77 合并，不重复立项。
- 更早一组报告沿用旧 JSON schema；主控负责把其逐源 EOF 凭证归一化，不据此重复扫描或扩大审查范围。此处不把运行测试、产品验收或 A 股官方规则复核表述为已完成。

## 汇总结论

- 多个历史 OOP 候选现已在基线 `43b1aa5` 有对应 owner/caller/consumer；对象或测试存在不自动核销 G/Q。批次 045、095、130、150、170、185、210、225 分别记录了该边界及相关反证。
- 现有总账缺口继续保持独立：G29 零 NPC 边界（045）、G31 图表数组引用更新及 G68 默认 setup/活动 setup 语义错位（130、150），以及其他批次提到的 G 项都没有因 OOP 保留或对象抽取而关闭。
- `PersonalPriceMemory::prune` 无生产 caller 的观察（135）已由主控归并至 G42，不另立重复项。
- `smoke-pages` 的 symlink 路径读取风险（105、080）作为独立工具链候选交主控归类；不能由 G39 验收约束或历史 OOP review 核销。
- Worker `load` stale/dispose、同步 `postMessage` 抛错后的清理延迟、Remote baseline generation 顺序边界（170、220）属于既有候选复核；需与 G66 等当前登记逐项去重，不表述为已确认运行故障。
- `CivilClock` 序号边界及 `CompanyOperationsClockWiring::sync` 部分写入（040、160）由不同批次重复观察，须合并为一组后按真实 caller 与现行 Q17/相关 ADR 评估。`write off` 的当前 caller/失败可达性证据有限，不从部分提交顺序推导端到端可达故障。
- 批次 175 的 D01 非正往来金额问题已登记为 G77；D02 虽有部分提交顺序线索，但所述写核销入口无生产 caller；D03 `close_year` 有生产日终 caller，具体失败可达性仍未证。Q17 的 `correct` 语义不替代这些核查。
- 材料级追溯问题保留为审计证据边界：批次 115 的 group-08 recheck/final-review 结论冲突；批次 140 的历史 handoff 指纹与指定 `domain.md` 不同；批次 195 的 session-07 绑定记录源文件 hash/行数与 `43b1aa5` 不同；批次 200 的 specificity delta validation 状态与 final 结论之间缺少裁定说明。这些均不能直接推成新的产品 G。

## 验收边界

本协调任务只核对历史材料、当前 baseline caller/owner/consumer 及其与现行 G/Q/ADR 的关系。未运行测试、构建、完整回归或浏览器验收；未重新核验交易所/中国结算官方法源。各批次原始结论及凭证见同目录 `batch-NNN.md` 与 `batch-NNN.json`。
