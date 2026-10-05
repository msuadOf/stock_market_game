# 股东登记基础模块独立复核

## 范围与证据

复核者未参与实施。已完整阅读 `cash-dividend-implementation-preparation.md`、`cash-dividend-share-registry.md`、`dividend-research.md`、`corporate-actions-research.md`、`AGENTS.md`、`docs/principles.md`、`docs/architecture.md`、`docs/open-questions.md`、ADR-0029，以及新增 `share_registry.rs`、`share_registry/tests.rs` 和 `company/mod.rs` 中仅新增 `pub mod share_registry` 的导出。另完整核对已归档的财税〔2012〕85号官方转载文本，未将该通知已被101号调整的旧税率当作现行税率。本次不运行 Cargo、不提交，不以作者测试转述作为本人实测。

此审查仅覆盖纯登记模块；Session、账户、税款、公司应付、支付、除息、SaveSlot 和宿主接线尚未纳入，不能据此判断 D01 已完成。

## 大 A 语义与必要范围

完整持有人股份合计等于发行股本、发行人自持不享有现金股息、外部持有人具名、取得日期和取得来源显式提供，符合当前已核实公司法第210条与现金分红准备契约。`IssuerTreasury` 在单发行人名册内定义发行人自持，不把其他公司投资当作发行人自持，且仍参与股数守恒。日终净增减而非逐笔成交消耗 FIFO，保留零净变动旧取得日；T+1不被误作无分红权。登记仅捕获当前已结日期，相同事件可重试但不能用当前持股补造过去登记权利，已登记快照不随后续卖出改变。

纯 Engine 模块、无额外依赖、候选副本验证后一次替换符合分层与失败原子性。当前只新增一个导出，不审查 `mod.rs` 中其他任务的会计税务导出。没有 schema 代际、迁移、默认取得日、默认税务身份或默认未模拟股东归属。

## 提交依赖与证据适用范围

纯模块分层独立不等于可在当前 HEAD 无依赖独立提交。已实际核对 `git show HEAD:packages/engine/src/orderbook.rs` 与工作区 diff：HEAD 的 `AccountId` 仍为 `js_safe_u64` 数字／TS number；工作区改为 `canonical_u64_decimal` 字符串／TS string。host40 十二项绿色证据来自包含这个尚未提交契约的工作区，MAX AccountId 序列化／恢复用例依赖它，不能外推为 A 模块单独放入当前 HEAD 后通过。

提交应先落完整 AccountId／成员／投影依赖批次，或者将 A 纳入同一个有内聚关系的提交批次。不得为拆分提交削弱 MAX 断言、给 HolderId 另造身份序列化或引入数字兼容。此处只确认真实依赖，不替代 AccountId 批次自身的独立复核，也不重新验证已提交树或完整工作区。

## 发现与修复复核

- **P1，混合限售与流通股份被错误阻卖：已修，host40 短测通过。** 原 `close_day` 遇未解禁旧 lot 就失败。当前 `PublicMarket` 跳过尚未解禁物理 lot，按 eligible lot 顺序消耗；mixed 名册构造与登记不被拒绝，明确解禁日达到后可转让，不足时返回带 holder／requested／available 的 typed error。A 返回物理变动证据，不冒充 B 的税务 FIFO。新增混合持股测试同时检查旧限售数量保留、较新流通数量减少、超额请求原子失败；解禁 guard 还检查恢复不可伪造未解禁转出。
- **已知 P1，同日消耗后的 lot identity 重用：已修并有 host39 绿色证据。** 在任何处置之前先查原状态完整 lot identity；同请求两次创建相同 lot 仍由 candidate 查询拦截。作者在初审前已登记此项，不计为新发现。
- **严格跨模块输入边界：已修，host40 短测通过。** `RegistrationSnapshot` 改为 `SnapshotState → TryFrom` 受检反序列化，与其公开 `validate` 共用身份／总量／取得日期检查。整个 Registry 仍额外校验证券、发行人、注册日期与已结日期关系。不存在直接反序列化绕过资格校验的已知入口。
- **完整字段契约：已修，host40 短测通过。** `acquisition` 使用无默认的 `deserialize_with`，保留明确 null 而拒绝缺键；没有建立旧格式补齐。`MovementScope` 必填，`NonTradingTransfer` 返回 typed unsupported；`PublicMarket` 净增必须 `SecondaryMarket + Unrestricted`，不能伪装开局、送转或限售取得事实。司法／继承等非交易业务不在本 API 支持范围，不据此缩减 D01 的最终目标。

## 测试边界与结论

已完整阅读当前十二项测试及相关实现；逐项阅读 host37 四份 exact 红测、host38 七份 exact 日志、host39 十份 exact 日志，未把 `--list` 当执行结果。host37 四项均业务红；host38 六绿一红；host39 七绿三红，分别是混合持股、独立快照、nullable 缺键。上述三项失败均与初审发现一致，不是编译或 fixture 伪红。

当前十二项用例覆盖 FIFO部分消耗、总股数、失败原子性、相同事件幂等、登记后卖出、自持排除、过去日期拒绝、MAX AccountId字符串、恢复额外／缺少字段及几类损坏、零净增减、同日 identity 重用、混合限售与公开转让、独立快照受检、明确 null 必填、普通市场取得来源／限制伪装、解禁日及伪造解禁恢复。额外建议的小 fixture 为总量溢出、空来源、重复 lot 与乱序日期、同事件不同登记日期；这些现有 guard 可源审查，但尚无独立执行覆盖，不以此要求完整回归。

最终逐项亲读 `.tmp/checklist-wave4/share-registry-host40-<case>.log` 十二份 exact 执行日志，每份均实际运行一项、通过一项、失败零项，单项显示0.00秒，未用 list 代替运行。该批由 root fresh Engine executable 执行；作者记录说明八进程并行、每进程 Rayon4、整命令10000ms及单项9000ms deadline，本复核只查阅日志，不另启动 Cargo 或复杂回归。

当前结论为 **纯 A 模块限定范围独立复核及十二项短测证据签核通过**。初审四个问题均已闭合，未发现修复引入新的 P1。签核仅覆盖本文件完整审查的纯物理登记模块，不证明 Session、税务 B 模块或三宿主生产接线，不得扩大为现金分红端到端或全部股本行为验收。
