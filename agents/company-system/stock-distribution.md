# 共同送股与转增股分配基础

日期：2026-10-06。

## 范围

新增 `packages/engine/src/company/stock_distribution.rs`，提供纯函数 `allocate_stock_distribution` 和 `packages/engine/src/company/stock_distribution_tests.rs`。输入只读 `RegistrationSnapshot` 及显式 `StockDistributionPlan`；计划分别标识 `BonusShares`（未分配利润转股本）或 `CapitalReserveConversion`（资本公积转股本），并要求 `approval_reference`、比例和获批新增股总数。比例使用百万分之一股为整数单位：`123456` 表示每股 `0.123456` 股，不经浮点换算。股数、ratioMicros 和 tie-break seed 使用规范十进制字符串，支持完整 `u64`，不依赖 JavaScript 安全整数范围。

算法按 `HolderId` 汇总登记日账户股数，排除 `IssuerTreasury` 的权益与碎股计算但在回执保留其被排除数量。其余账户逐户计算 `持股数 × ratioMicros / 1_000_000` 的 floor 和余数；获批总股数同时必须落在逐户 floor 总和至逐户 ceiling 总和，以及 eligible 总持股数乘比例所得 floor 至 ceiling 的交集内。这样既不做每户四舍五入，也不能借碎股补整把总获批股数扩张到比例总量之外。差额按余数降序逐户补 1 股，同余数由显式 `tie_break_seed` 驱动的可复现洗牌排序。结果中保存方案、登记快照身份、每户基数/余数/整股数以及原 lot 完整只读快照。

回执显式标记 `SourceLotAttributionPending`。分配基础不创建、修改或分摊 `ShareLot`，不生成新 lot 可交易状态，也不伪造送转后税务持有期限。之后的结算必须先取得 lot 来源、税期限及限售传承规则证据，再将账户级奖股转换成 registry 变更。深市指南明文确认限售送转股继承原限售性质和截止日；沪市本轮未定位对应条文，不能跨市场推断。

## 官方依据

按研究记录中的原件全文核对，依据《中国结算上海分公司证券发行人业务指南》（中国结算沪业字〔2024〕6号，2024-01，2.5节）、《中国结算深圳分公司证券发行人业务指南》（中国结算深业〔2025〕68号，2025-12，2.5节）及中国证监会公告〔2023〕63号《上市公司股份回购规则》（2023-12-15施行）第13条。指南 PDF、正文摘录和哈希在 `.tmp/company-system/corporate-rules-followup/`；回购规则官方 PDF 及全文在 `.tmp/corporate-actions-research/repurchase.pdf` 和 `.tmp/corporate-actions-research/repurchase-pdf.txt`，官方来源与适用日期记于 `agents/remaining-questions-and-features/corporate-actions-research.md`。

沪市指南要求每股送转比例最多六位小数，碎股以投资者账户碎股数降序、同数由电子结算系统随机排列，然后依序各登记一股直至完成。深市指南的权益分派申请比例最多六位小数，并规定相同的账户碎股排序；另明确限售股份孳生的送转股仍为限售股且截止日与原股份一致。两地回购专户排除具有共同上位依据：证监会公告〔2023〕63号第13条规定，上市公司回购股份过户至回购专户之日起即失去权利，不享有利润分配、公积金转增股本等权利。模块将随机排序 seed 作为显式游戏输入，使用 SplitMix64 与拒绝采样的均匀区间抽取作可重现模拟；官方材料只规定系统随机，不规定离线 PRNG 或种子。

`HolderId::IssuerTreasury` 是 Registry 中本发行人自持股份的权威持有人身份，与 `Account`、`External` 一样是游戏当前登记事实；它不证明持仓的历史取得来源，也不代表真实账户资金流水。`BonusShares` 的自持股排除依据为《公司法》第210条，可由该明确类型身份表达；`CapitalReserveConversion` 的排除依据为证监会公告〔2023〕63号第13条，要求额外证明股份已转入回购专用账户。`ShareRegistry` 现保存必填 nullable 的 `issuer_repurchase_account` 事实 DTO，要求账户引用、来源凭证和建立日期；恢复时拒绝缺键，事实日期不得晚于 registry 已结算日，已有登记快照时不能回溯事实建立日期。登记时仅在事实已于登记日建立的情况下，将其复制到不可变 `RegistrationSnapshot`；恢复时校验顶层与历史快照资格事实按日期一致。分配逻辑对 `CapitalReserveConversion` 只在快照有此事实时排除 `IssuerTreasury`；`BonusShares` 按公司自持股规则排除，不以回购专户事实扩大或缩小该规则。

## 验证状态与限制

模块声明已由 root 加入 `company/mod.rs` 并公开；此模块仍未接入 `Session` 或 `Finance`，不代表真实送转登记结算完成。

测试源码覆盖账户级 floor/碎股补整、`IssuerTreasury` 排除及其不参与比例目标、seed 平局可复现、逐户与汇总审批总量可行区间、超过 `2^53` 的股数、u64 最大 ratio/seed 字符串往返、资格事实恢复缺键拒绝、日期校验和一次性登记，以及审批引用/零比例错误。root 观察 `source-and-announcement-red-0.log` 与 `source-and-announcement-red-1.log` 时，两条缺少 nullable 事实字段的 Registry/Snapshot 恢复负例均按预期失败；`source-and-announcement-red-4.log` 中送转基础 8/8 通过。千户汇总总量负例此前暴露原实现只约束逐户 ceiling 的问题，随后实现同时约束 eligible 总股数比例 floor/ceiling 和逐户 floor/ceiling 区间。当前基础仍未接入 `Session` 或 `Finance`，不代表真实送转登记结算完成。

最新统一编译后的 Registry 21/21、送转分配 9/9 短测通过，证据分别为 `.tmp/company-system/session-actions/final-registry.log` 与 `final-allocation.log`；Web 对应严格存档解析 11/11 通过，证据为 `latest-web-registry-green.log`。各组外部 deadline 均为 10000ms，Rust 组内 8 线程、组间并发。修复非法零股本测试 fixture 后，仍验证普通账户持有全部发行股份、没有 `IssuerTreasury` 持仓时可以登记专户事实；非作者增量复核已确认该修复没有弱化断言。

余项：已补查送转 lot 税务期限继承以及沪市限售继承证据，研究明确尚不能确定混合来源的逐 lot 分配。取得规则并形成独立的明确来源分配契约之前，账户级回执继续保持 `SourceLotAttributionPending`，不把上述算法与解析短测表述为实际送转结算完成。
