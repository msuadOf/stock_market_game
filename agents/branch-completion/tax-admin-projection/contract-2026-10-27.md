# 税务 Admin 候选投影最小合同

日期：2026-10-27。状态：待原未实施 Sol 确认，不是中央写窗许可。
真实 main 为 `1f4c8fe6daca242e8845ea2cd07a8c2fea41756d`；K2ab 已获 native105 域门批准。
本合同落实已审个人 Bonus v3 与 K2 v2 的 Admin inspect 接缝，不新增税务日或收缴优先级。

## 唯一来源及范围

- 输入借用同一税扣款阶段的真实 before 账户快照、原始 grouped `AdminOperation` 和真实 `AdminCandidate`。
- `AdminCandidate.groups.accounts` 只有候选 after；before 必须来自该阶段权威快照，不能用 Session 旧镜像、上一日账户或自己声称的实收替代。
- 本首片只接受由 `WithholdingTax` 构成的非空税阶段；混合 CashDividend/ShareSplit/其他操作明确拒绝，不复制通用资产 dispatcher。
- R4 writer 已明确本叶由税务 owner 独占；其 stage 索引 join 不计算金额、不代替本校验。当前只有真实 tax-only 整事务可提供同阶段 before/after；混合 R4 尚无税阶段账户 producer，不能拿全事务最初 before 比混合阶段最终 after。
- 原始 typed ops 是扣款请求的唯一来源；不另收一份可冲突的总扣款金额，不从 state_hash 或现金差猜税款。

## 最小校验及输出

建议新增未注册叶 `runtime/admin_tax_projection.rs`，`pub(crate)` 只读 validator 返回 prepared 投影或带 group/local/stock/operation index 与具体数值的错误。
Rust 名称可在测试时缩短；不新增 protocol variant、全局 ID、持久化 receipt 或另一事务。

1. 输入组/候选组、每组触达 local 必须精确对应；拒绝重复组、重复/缺失/额外候选 local，不能先放入 map 覆盖重复。
2. 每个 operation 必须为正分额 `WithholdingTax`，group/local/stock 存在。依原组内操作次序，用 before.raw cash checked 扣除；同账户跨证券扣款共享现金，不按股票隔离余额，也不允许后续款项遮盖先前不足。
3. expected 从真实 before 完整 clone 后仅修改 cash，再与候选完整 Eq；buy_purse_held_cents、shares、frozen_today、foreign_active、invested_cents、recovered_cents、ever_held、pending_sell_qty 及所有现有字段均保留，不用局部字段比较冒完整资产勾稽。
4. 返回 P 或其私有 wrapper 绑定真实 candidate.id、输入 group 位置与组内 operation index、坐标与原分额，形成不可变 prepared 投影。不生成 TaxIncomeKind/税 event，不序列化，不设置 committed 标记。保留逐条来源顺序，不自行合并分 kind，不自造 id，也不把 `AdminTransactionId` 变成持久税事件 ID。
5. inspect 失败不发布、不安装、不修改输入。只有 `World::with_admin_transaction` commit 成功返回 P 后，R4 才可将该 prepared 投影对应的受控税务事实发布。叶本身不能证明 commit 已成功，不标记为 collected receipt。

## TDD 与接线出口

fresh Luna/high 在独占新树实施真实源码/fixture，先保测试源码快照与可编译 stub 的业务红，再最小实现。
测试使用真实 engine 类型；至少有一组真实 World prepare/inspect/commit 成功与 inspect 失败不发布/账户不变，禁止自造 AdminCandidate.id。
其余负例可在真实 inspect 内克隆候选后篡改，用真实 before/typed ops 校验：错金额、改 buy_purse_held_cents/持仓/成本/冻结/在途、缺/多/重复 group/local、非税操作、零负额、同账户多证券累计不足、非零 group/local 和空/越界。
失败原件保留；普通构建/执行各 10000ms 外部进程树门，多核参数明确；不启动 Cargo 或新全 engine 构建，不冒已密封 normal rlib 为新 main 构建。
完整 diff 与红绿绑定由原未实施 Sol 审。首片只批准真实 World tax-only 整事务接缝；混合 R4 必须等同 pending、同 id、组端真实税阶段 apply 前后的账户只读 producer 设计和实现独审。索引 join、手工重建假 AdminCandidate 或 prepared 金额不能冒该来源。纯 core 可复用，随后仅可信阶段 adapter 薄接，不复制算术；接口/fixture及精确最小中央 hunk直接交R4排窗，不作为长期独立 helper 终点。

真实来源反例：事务初始 cash0，stageA 股利+100，stageB 扣税10，最终90。税阶段真实before100→after90；不能拿初始0与最终90核tax10。当前 `AdminCandidate` wrapper 只承诺 tax-only 整事务；未来 stage wrapper 待来源合同冻结后收敛，直接消费真 id/stage/before/after，不为旧签名重造 AdminCandidate，不提前建通用 trait/dispatcher。

## 明确未闭合依赖

REG `23759efa` 只提供物理 phase/capture；SOURCE-B `f9c1b417` 只纯资本恢复，无税务日/原权益 producer 或 live caller。
下一实际生产门是 `TaxDayBoundaryFacts`、`TaxRegistrationAttribution` 的真实来源与 R2 credit/所得激活，以及分 kind 收缴受控来源。
SOURCE-B 唯一 writer 提供 selected system/actions 与 saved enum；税务 owner 与 REG/R4 同窗补 producer。strict save 唯一 owner `/root/sol61_plan_identity_runtime`。
中央顺序 SourceA→#122→C2C→R4；本叶不写其中央文件。R2 `UnsupportedPersonalStockBonus` 保留，不能凭叶通过撤门。
