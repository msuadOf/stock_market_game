# Simple 公开材料独立复核

## 复核范围与阶段

本次由未实施者先完整阅读旧方案的报告内容、公布、估值和测试文件，再复核用户新蓝图后的实际 diff、`source_tests.rs` 与更新后的 `information-contract.md`。旧 partial 材料已精确撤回；当前共同完整 `ReportSet` 仅增加必填 `PublicationSource`，`publish_simple_closed` 复用真实 ClosingEngine 定稿公布。本记录仍为实现阶段复核，不表示短测或生产接线完成。

## 领域与必要性

用户最新要求 Simple 与 Simulation 共享完整上层财务功能，差别在内部经营形成机制。撤回分模式 partial 报告和可选 AnnualFacts 的修改，保留共同 ReportSet、既有期间及会计金额元字符串，是必要且最小的适配。必填来源区分汇总生成与经营仿真，不冒充真实客户经营经历，不构成旧格式兼容。

盈利、权益 ROE 和现金流分析继续使用同一已知报告，来源不能用来禁用某种分析或替换方法。Simple 报告是否实际勾稽、月度形成和公开排期是否接通，仍需 finance 与 Session 所有者提供证据，不能由来源标签推断完成。

本人材料获取继续通过 `NpcInformationState` 与 `NpcObservationContext`；公开材料的生成不冒充本人获知。已有公开时点守卫保留，玩家与 NPC 不能因内容模式变化提前读未来报告。

## 必须修复的发现

- **版本与更正恢复边界：** 实际修改已新增共同 `validate_correction_target`，恢复按 PublicationId 排序，再校验目标存在、同公司／期间／种类／范围／来源、时点及递增版本。版本关系须区分私有登记序号与公开 ID：尚未公开的私有 Original 可以具有较大序号，Correction 的私有前序可以高于公开目标版本，不能强制两者相等。当前采用公开目标序号 ≤ 私有前序 < 新序号，方向合理，尚应加入合法私有前序高于公开版本的正例。
- **来源在更正链漂移：** 实际修改已在普通公布入口拒绝原目标来源与请求入口不一致，原子更正继续继承原来源；恢复链也校验来源。旧路径的两个真实失败已见 `.tmp/checklist-wave4/host55-information-source-red.log`，修复后新的生产短测尚待提供，不以源码一致代替绿证据。
- **无公开目标的版本形状仍待确认：** 当前目标校验在公开 supersedes 为 None 时直接成功。应明确无公开更正目标时允许哪些私有 VersionKind；若只允许 Original（即使序号大于一），需校验该种类及私有 supersedes 为空；若允许私有 Correction 首次公开，则需明确记录该语义，并以真实 ClosingEngine 路径短测证明，不能根据私有／公开编号不同而自行收紧错误规则。

`insert_report` 已将重复 ID、checked ID 后继、形状、目标和候选 digest 计算前置到插入之前，失败不改报告及索引，且不为求原子复制全部报告库。这一根修符合防御式编程和性能约束；最终短测仍需覆盖成功恢复 digest 与直接公布一致。

旧方案的 Simple DTO 缺失版本字段问题已随 partial payload 撤回而失效；当前共同 `financials` 已承载版本类型、理由和版本 supersedes，不应为旧发现再增冗余顶层字段。

## 后续短测与完成门禁

现有两个来源测试尚不足以证明版本及来源链边界。最终复核需确认：失败公布不消耗 PublicationId、不改索引／digest；更正目标、版本和来源异常在直接公布及恢复均被拒绝；缺失 source 的存档明确失败；Simple 真实定稿年报仍携带完整 accounting 与 financials，DTO 来源直接来自报告。月度材料到年报的完整窗口聚合、财务真实勾稽与排期由接线所有者另行证明，不能由纯来源测试推断完成。

只需代表性短单元测试和完整实际 diff 独立复核，不要求复杂回归。修复后本记录需按真实实现及测试证据更新，再作最终结论。
