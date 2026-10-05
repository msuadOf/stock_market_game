# Simulation 设计文档入口迁移独立复核

## 范围

仅审查旧 `packages/engine/src/company/economy/system_sim/{README,DESIGN}.md` 删除、新 `packages/engine/src/company/simulation/{README,DESIGN}.md` 建立，以及 `blueprint-independent-review.md`、`company-checkpoint-review.md`、`q14-financial-model-design.md` 的相关路径与历史措辞更新。不审查并行源码改动，不运行测试或回归。

完整比较旧、新两份设计文档至文件末尾：对旧内容应用预期的相对路径变更后，README 正文逐字相同；DESIGN 除首段“目录目前仅有文档”准确更新为“仅有设计文档”外，其余正文逐字相同，路径及已执行迁移状态措辞按本次变更调整。没有发现截断、遗漏尾段或额外内容漂移。

## 独立审查

1. **大 A 语义与依据：通过。** 本次只移动设计文档和修正路径、历史状态描述，没有新增或改写沪深市场交易制度、会计确认规则、股东结算条件、单位或默认参数。原有分红账面现金边界、有限现金仿真边界及“未实现规则不得冒充支持”等说明保持原文。没有引入新的规则主张或需要新增官方依据的内容。
2. **必要性与范围：通过。** 将持久设计入口迁到目标 `company/simulation/`，并同步现行蓝图及两份审计记录的历史路径语境，是消除过期阅读入口所需的最小改动。新旧文档声明本次没有移动运行时代码，也没有注册 runtime `simulation`；未发现兼容别名或额外运行路径。
3. **边界与引用：通过，附一项记录时效提示。** 所审文件中的本地 Markdown 链接目标均存在。新 README 与 DESIGN 的向上相对路径及 DESIGN 中 operations、scheduler、accounting、session 链接与新目录层级一致。旧目录下无残留文件。`company-checkpoint-review.md` 保留的旧路径清单有明确“历史文件清单”说明，属审计时点证据，不是当前入口。

**审计时点提示已由后续修改澄清：** `agents/company-system/checklist-current-code-audit.md` 第 21 行记载的是 2026-10-06 审计时点的旧路径缺口。实施清单随后将持久设计入口迁移项勾选，并明确仿真运行时代码仍属于后续分支；两者指向不同交付范围，没有把文档迁移冒充为运行时实现。

## 结论

指定迁移差异通过独立复核；未发现大 A 语义漂移、设计正文丢失、断链、compat alias 或 runtime 注册。实施清单对迁移范围的勾选与后续运行时代码边界表达清楚。未运行测试/回归，亦未审查其他并行源码。
