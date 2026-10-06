# 送股与转增股分配独立复核

日期：2026-10-06。Reviewer 未参与实现；按要求仅审阅，不运行 Cargo、不修改 production。范围为 `packages/engine/src/company/stock_distribution.rs`、`stock_distribution_tests.rs`、`company/mod.rs` 中模块声明及 `agents/company-system/stock-distribution.md`。未将工作区其他并行变更纳入本次结论。

## 结论

- **先前阻断已关闭：`IssuerTreasury` 排除具有全国适用依据。** 初审只查了沪深中国结算操作指南，遗漏适用的证监会公告〔2023〕63号《上市公司股份回购规则》第13条。官方原文规定上市公司回购股份过户至回购专用账户之日起失去其权利，不享有利润分配、公积金转增股本等权利；因此共用算法排除回购专户股份的 A 股语义成立。初审报告对此依据覆盖不足负 reviewer 责任，原阻断撤销。
- **资格接线复核通过。** 根已明确领域契约：`HolderId::IssuerTreasury` 是 Registry 中本发行人自持股的权威 holder identity（与 `Account`、`External` 同层），不表达取得来源或现金流水；故《公司法》第210条足以支持 `BonusShares` 排除，无须回购专户 facts。`CapitalReserveConversion` 属额外法定类别，代码要求有专户 facts 时才排除 issuer treasury，且缺 facts 时拒绝。新增 `IssuerRepurchaseAccountFacts` 保存 account reference、source evidence、established date；set 操作校验非空、不能未来设定、已设定仅幂等重设同一事实，并拒绝跨现有登记快照回溯 established date。注册时按当日事实复制到不可变 snapshot，snapshot/registry 单独恢复都验证日期。
- **历史快照一致性修复复核通过。** `ShareRegistry::validate` 现按日期推导每个 snapshot 应有的 facts：顶层事实存在且 `established_on <= registered_on` 时必须完全相等，否则必须为 `None`。这符合设置前旧快照不变、设置后新快照复制事实的时间模型；setter 禁止回溯到已有快照日期或之前。新增 restore 负例篡改旧 snapshot facts 后拒绝，API 负例验证回溯 setter 原子拒绝。此前矛盾历史资格信息阻断已关闭。
- **已关闭：序列化与 seed/余数排序的主要测试缺口。** 最新测试覆盖 ratio/seed `u64::MAX` 规范十进制字符串往返，numeric/leading-zero ratio、未知 plan 字段、receipt 未知/缺字段拒绝；覆盖不同 seed 结果差异、holder 输入顺序反转稳定性以及最大余数优先。`SeededRandom::index` 使用 SplitMix64 与拒绝采样；拒绝阈值 `2^64 mod upper` 以下值后取模可消除模偏差，Fisher–Yates 限于同余数分组，不改变余数优先顺序。尚未固定 PRNG 向量，但当前不作为阻断。
- **非阻断：fresh green 尚未执行。** 本复核未运行 Cargo；委托说明 root 已观察 `stock-target-red.log` 负例按预期失败，fresh green 尚缺，不能据此报告实现通过。

## 语义、依据与数学

- 沪深两份中国结算《证券发行人业务指南》均规定送转不足1股碎股按持有人碎股数量降序，同数随机排序，再依序各登记1股直至全部完成。代码按聚合后的持有人计算余数、余数降序，并对同余数段使用显式 seed 洗牌，符合碎股分配机制；官方材料没有规定模拟系统的 seed 算法，文档已说明这点。
- 两份指南均将送转比例精度限定为小数点后最多六位；`shares_per_existing_share_micros` 以百万分之一股表示，整数类型能表达该精度且不引入浮点舍入；本次更新把 ratio 和 `tie_break_seed` 也改为规范十进制字符串，能无损表达完整 `u64` 并避免 JavaScript 安全整数边界。代码只拒绝零比例而未设比例上限；以整数微股语义看，正比例大于1股/股本身不构成精度违规，方案合理性留给获批总量约束。
- 每个 holder 的乘积使用 `u128`，逐 lot 汇总、eligible 股数、逐户 floor/ceil 聚合及 receipt 奖股增量均做 checked 运算；在输入字段为 `u64` 时乘积最大值仍可由 `u128` 容纳。获批总量同时落在逐户 floor/ceil 和 eligible 总额 floor/ceil 的交集内，随后按余数降序仅对正余数 holder 各加至多1股。这个交集避免“千个1股持有人、10%方案却批准1000股”一类按户 ceiling 汇总造成的总量扩张，符合总比例边界。
- `RegistrationSnapshot::validate` 是入口防线；函数按 holder 聚合 lots，保留 holder 的原始 lot 拷贝但不创建或更改 lot。`SourceLotAttributionPending` 明确阻止把账户级奖励伪装成已确定 lot 来源。当前没有把送转股份接入 registry，因此未擅自决定沪市限售继承，也未赋予税务持有期；深市限售送转股继承原限售属性/截止日的明文不能跨市套用，研究文档已正确标注。增量研究确认个人股息税相关持有期规则不能替代送转新 lot 的逐 lot 来源分配规则；若属于法定限售类别，应按相应官方依据区分，不能把 contract-only restriction 混同法定限售。登记及税务计算接线前仍须形成明确 attribution 契约。
- `CapitalReserveConversion` 与 `BonusShares` 目前仅作为计划种类保存，分配算术相同；没有声称它们的会计、税务或历史取得成本相同。该模块不处理税额或税务持有期，现有研究明确相关税务规则证据不完整，此范围是诚实的。

## 必要性与最小性

Q14 第二章要求共同公司行为规则复用于 `simple` 与 `simulation`，公司股本行为使用真实股份数量；实现纯函数、读取登记快照、返回可序列化回执，属于该目标的必要基础。`company/mod.rs` 只加模块声明，改动范围与交付一致；没有新增依赖、没有接入 Session/Finance/ShareRegistry，也没有把未完成结算宣称为已完成。

回执保留计划身份、登记快照身份、排除 treasury 数量、分配结果和原 lots，且将来源归属标为 pending；相对于可审计纯计算结果是合理的，但 `original_lots` 是重复快照且会扩大回执体积。当前尚未持久接线，保留可帮助后续来源映射复核；接线后应确认不再无必要地重复持有整份 lot 数据。

测试已覆盖逐户 floor、碎股奖励、treasury 排除、逐户和聚合总量上界、超 `2^53` 精度、审批字段和零比例、ratio/seed `u64::MAX` 字符串往返及反序列化负例、不同 seed、holder 输入顺序无关和最大余数优先。Registry tests 覆盖必需 nullable key、快照复制、仅同值重复设置、空/未来日期拒绝、回溯建立日拒绝、篡改历史 snapshot facts 后 restore 拒绝；分配测试覆盖 CapitalReserveConversion 有/无资格 facts 分支。剩余非阻断测试建议：有效 Some facts 的 registry/snapshot 完整 wire round-trip、empty/全 treasury snapshot 且 approved total 0、多 lot 同 holder 聚合、极限 `u64` 可行/溢出 case。

## 复核门禁状态

增量复核后，上轮两项阻断均关闭：`IssuerTreasury` 权威 holder identity 与公司法第210条一致；专户资格事实、时间约束、不可变 snapshot 复制及 Registry restore 历史一致性校验均符合该模型。当前指定范围未发现新的阻断性语义问题。仍有有效 Some facts 完整 wire round-trip 等非阻断测试增强建议。fresh green 由实现方另行确认；本 reviewer 没有运行 Cargo 或验证红绿结果，因此本记录不证明测试已通过。

## Fixture 增量复核

root 指出的 `no_holding` 测试 fixture 原先以 `issued_shares = 0` 构造 Registry，与 `ShareRegistry` 的非零股本校验冲突。最新改动将其替换为 100 股由普通账户持有，并断言无 `IssuerTreasury` holder 时仍可设置 issuer repurchase account facts；这保留了测试“专户事实可在尚未持有回购股份前登记”的意图，也满足股份守恒和正股本约束。该修复无生产逻辑变化，本次未运行 Cargo。
