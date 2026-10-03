# 批次 082 独立复核

## 阅读完整性

产品源码基线为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`（`.worktree/implementation-reaudit`）。三个来源均从主仓连续全文读取至 EOF；实测行数与计划一致，SHA-256 匹配。

| 来源 | 行数 | SHA-256 | EOF |
|---|---:|---|---|
| `agents/oop-refactor-audit/exhaustive/reviews/web-03-date-final.md` | 16 | `a68c4ae020c9a23baf5946d18b1b03a3a6bfacaecbef8481974dfafe3f085b8b` | 是 |
| `agents/oop-refactor-audit/exhaustive/reviews/web-03-parse-final.md` | 28 | `ef4b4ddeb507cc56c96ddc5eff990419ac47a6925c0b75d43bbabe799a867e41` | 是 |
| `agents/oop-refactor-audit/exhaustive/reviews/web-04-remote-final.md` | 19 | `d08fe8a8d1f262d4c0cde42773a0233d1d843456ddd541b543f7a88aac504bdd` | 是 |

已读主仓 `AGENTS.md`、`docs/principles.md`，以及基线工作树中的审计主账、coverage index、相关 re-audit、ADR-0010、ADR-0027/0028 和当前 open questions。没有运行测试/构建，没有 Git 写入，也没有查交易所规则。报告仅写入指定 hidden-review 目录。

## 章节矩阵

| 来源章节 | 当前 caller / owner / consumer | 结论及候选、反证 |
|---|---|---|
| web-03-date-final：unit083 `nextDate` 的日期边界 | `validate.ts:128-148` 的 `validateCivilUpdate` 调用 `nextDate`；未知输入生产路径由 `parse.ts:204-207` 解析 CivilUpdate，`wire-values.ts:34-45` 的 `parseIsoDate` 检查格式、真实日历日期及 1900–2199 范围，之后进入 `validateEngineUpdate`（`validate.ts:199-205`）。 | 保留结论成立：`nextDate` 是纯日期校验辅助函数；其 `Date.UTC` 归一化只校验相邻民用日，不承担交易日历校验。typed DTO validator 单独调用时确有前提，但不能据此宣称生产 parser 接受无效日期。必要候选 OOP 动作为零。反证/限制：直接构造畸形 `CivilUpdate` 调用 validator 的边界仍未由 parser 保证；若要求 validator 独立防御，属于新边界需求，不由此次 OOP 复核推导。 |
| web-03-parse-final：四组 wire 常量清单 | `parse.ts:31-41` 定义 `EVENT_NAMES`、`EVENT_SOURCES`、`REJECTION_REASONS`、`CIVIL_KINDS`；实际消费点为 `isEventName`/event switch（`:47-123`）、`stableKey` 的 enum 校验（`:69`）、拒绝事件解析（`:107`）和 CivilUpdate `kinds` 解析（`:204-207`）。 | 保留结论成立：这些是无状态 wire 标签 allowlist，生产解析点消费明确，不持有生命周期状态。未发现常量应搬入对象的必要候选。`SettlementError.reason` 保持文本、`IntentRejected.reason` 使用有限标签，二者不可合并；标签清单闭合也不证明所有 parser 行为已有运行期测试。 |
| web-04-remote-final：启动暂停偏好与 generation | Web lifecycle 在 `useSessionHostLifecycle.ts:136-140` 先 `setPausePreferences` 再 `start`；RemoteHost 在 `remote-host.ts:192-195` 无 baseline 时用 generation `"1"`。远程会话创建在 `:30-45`，baseline/generation 状态由 `RemotePublisherState` 持有（`remote-publisher-state.ts:13-14,52-70`）。 | 新局前置偏好使用 `"1"` 的历史争议已由材料中的 Server 跨域核实解决；当前没有必要新增 OOP 封装。历史材料未覆盖的 host 实际启动时序测试仍是测试证据限制，不是实现缺失。反证/未决：`remote-wire.ts:53-68` 解析 Baseline 时校验 generation 形状/范围，但未与 active generation 比较；`RemoteHost` 的 Baseline 安装会替换缓存并递增 epoch，delta generation 不匹配则 resync。baseline 乱序/旧 generation 到达行为仍是协议边界未决项，不据当前代码决定应拒绝、忽略或接受。 |

## 总账、决策及语义关系

- G/Q 总账没有被本批任一来源核销。Remote 的启动时序事实只解除旧材料对新局 generation 1 合法性的疑问；它不修复总账 G01–G05、G18–G20 等远程宿主断点，也不改变 G04 对暂停后重复 `start` 重发旧 baseline 的现行记录。Web-03 日期/parser 的 OOP 保留结论不形成新 G 或 Q。
- 开放问题 Q01–Q12 与本批无直接待决项；历史材料提及的 Q 不能覆盖其现行 ADR 决策。没有发现可据此新增产品缺口或主张账目核销。
- ADR-0010 仍规定 baseline 用于初始化、读档和显式重同步，并说明 Remote push/pull、重连、鉴权等宿主契约；ADR-0027/0028 是较新的运行/发布决策，没有退役或改写本批日期 parser、wire 标签和 Remote generation 契约。ADR-0017/0018 的撮合规则与本批无直接关系；ADR-0018 整体仍为 proposed，不能从中导入额外约束。
- 本批没有交易规则改动。UTC 日期运算表示协议民用日连续性，不等同交易日历/开市日；generation 和 wire enum 是传输协议信息，不形成证券类别、撮合或受理优先级。无需重新查询官方 A 股规则，也不应将 OOP 提取或静态测试材料冒充产品行为缺失/运行通过。

## 结论与限制

三份来源的 retain/既有处置均有当前调用关系支撑；没有发现待实施的必要 OOP 候选。日期边界、事件 enum、remote generation 的现有分工保持在纯 parser、validator 与 host 生命周期 owner 内，抽象搬迁没有充分必要性。应持续保留两个明确证据边界：直接 typed DTO validator 的日期前提，以及远程 baseline 乱序语义/缺少真实 host 前置时序运行期测试。此次为静态复核，未运行测试、构建或官方规则查询。
