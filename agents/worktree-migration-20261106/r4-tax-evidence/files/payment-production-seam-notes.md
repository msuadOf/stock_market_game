# 付款实际首红之后的接缝备忘

仅设计，未实施；必须先取得 `779a7f91` 三门现金 marker 的实际 red。root 要求主要精力用于真实阻塞，不重审已经通过的基础设施与不变源码。

- `civil_information` 的 ordinary／capture shadow 成功，完成原 lazy adoption 后，首 Prepare 前只读一次现有 `AdministrativeBaseline`；以其真实 `GroupAccount.cash` 作为收缴输入。不得用含 buy purse 的 Session 总现金替代。
- 付款叶只调用现有 `distributions_day_end_candidate`，不新算税额。以其 company candidate 产生最终 INFO facts；任何 `StockDistributionCredit` 在本现金片显式拒绝，不顺带接通股份 movement、BonusShares 或来源不足的资本公积。
- 同一 baseline 传入既有 `apply_administrative_scope`；首 Prepare 前比对实际 `WorldMirrorSource` 与 baseline.source，并复用原完整镜像校验。不能再读 working 或复制一套 scope。
- World 增加保留 `PreparedAdministrativeSource` 和 `PreparedMirrorAck` 的 prepared staged 薄入口；raw staged 只适配它，仍在同一个 `with_administrative_inspection` 内调用，不多造 pending/id。driver 仅转发。
- cash intents 按原意图顺序的连续 gross／tax-only 段分 stage；不能把所有 gross 无条件提前，以免 Flat 多笔接近 i64 上限时改变原 checked overflow。Individual 通常就是 gross 总段→collection 税段。零 collection 不发送零额 WithholdingTax。group/local 从 baseline 完整真实 roster 与 per_group 解析，不从 AccountId 数值猜。
- inspect 借原 stage operands 逐一 join 原 distribution intent 的事件、股、账户、分额与 flat index；税 projection 同 candidate id／stage／operation 索引，保留实际税前后 raw cash。完整最终 GroupAccount 与基线副本仅 cash 改变的预期值相等，不能只比最终净额。
- 原 source/receipt 与候选公司 paid facts 均在首 Commit 前核验。候选 Companies／Paid／个人 payment／collection 只由原 `PreparedCivilFields` 固定成功尾移动；镜像 ack 仍由唯一安装者消费。
- `CivilInformationError` 已有 opaque Error 容器，可添加内部 typed source 构造而无需新建宿主协议或 SessionError 变体；保留候选原 typed cause，不压成字符串。通讯/协议失败仍沿 terminal，业务 inspect Abort 不安装付款事实。
- 已有三门只证明个人 gross、简税 net 与真实售后 collection；生产之后还须以同事务最终 projection 拒绝验证付款事实和账户均无泄漏，不能把成功门当失败原子性证明。
