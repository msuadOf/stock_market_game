# LiveReservations 独立静态复核（2026-10-06）

## 结论与范围

**本批生产逻辑静态复核通过，未发现阻塞问题。** reviewer 未参与实现，只读取源码、文档和工作记录，没有执行测试、Cargo、perf、GDB 或浏览器。此次通过只覆盖以下六个生产文件相对 `c06151ac` 的完整改动及其新增测试；不表示 ≥1000x 目标、WASM、HTTP/WS、全交易日或 frontend 已完成验收。

- `packages/engine/src/session/account_book.rs`
- `packages/engine/src/session/pipeline/decision_resources.rs`
- `packages/engine/src/session/pipeline/ledger.rs`
- `packages/engine/src/session/pipeline/ledger_paged_map.rs`
- `packages/engine/src/session/pipeline/live_reservations.rs`（新增文件，包含五个测试）
- `packages/engine/src/session/pipeline/mod.rs`

已阅读 AGENTS、principles、trading-rules、open-questions、architecture、ADR-0017，以及 ADR-0018 的状态、页级 COW、交易约束、摘要与待裁决边界。ADR-0018 仍为 `proposed`，其中用户已裁决的局部冲突规则与未获接受的观察版本、持久化等建议不混用。本批没有实施新的观察版本或持久化承诺。历史 untracked 日志、已撤回 CountedRows/math helper/page proof 不在本次复核范围。

## 1. 大 A 语义与依据

符合本项目已登记的沪深 A 股领域口径及明确的游戏简化。此次是 live envelope 的等价派生与缓存优化，没有新增或调整交易制度，因此无需用新的非官方假设替代现有依据。

- `Money` 仍以分表示；shares 仍为股。现金按账户累计，股份按账户及 `StockCode` 累计，cash 与 shares 不混合，同股票的不同账户股份也不相加。`live_reservations.rs:142–158` 的账户切换与逐股票 `checked_add` 保持旧 fold。
- seal 入口与时点没有移动，仍读取 ExpiryShadow 之后的 live ledger，不消费 terminal evidence，不把密封批释放回补当批资源。available cash、T+1 后可卖数量、权益与费用计算均保持原实现。
- Buy 的零 shares 股票条目保留，避免未知 `StockCode` 被零值过滤。`decision_resources.rs` 仍先完成账户资源校验，再报告未知 account，最后报告未知 stock；全局未知引用检查在每次 seal 均执行，不能被 AccountBook 页缓存跳过。
- envelope 仍是游戏内资源占用模型，不将卖单零现金预留或费用所得封顶描述为真实券商清算。

可靠依据为 `docs/trading-rules.md` 中已登记的现行沪深交易规则、中国结算与财政部来源，以及 accepted ADR-0017 的资源分配与守恒契约。交易规则文档记载 2026 版规则自 2026-07-06 施行，相关一般条款核对日期为 2026-09-22，竞价优先条款为 2026-09-25，数量条款为 2026-10-05。中国结算费用表访问失败与沿用较早核对记录的限制仍存在；本次没有重新访问官方站点，也没有宣称重验全部真实制度。

## 2. 必要性与最小范围

改动针对已有诊断中 SealAllocationSnapshot 每 tick 重建两张全局预留 map 的成本。新增 `LiveReservations` 是其唯一替代派生表示；ledger 缓存、decision consumer、AccountBook 页依赖及 module 接线均为此所需。没有新增 dependency、公开 API、save 字段或无关交易行为。

AccountBook 与 LedgerPagedMap 共用 `RESERVATION_PAGE_SHIFT=5` 合理：两者之前已按相同的 32-account 范围组织页，AccountBook 的完整预留依赖必须与对应 projection 页对齐。统一常量避免未来独立调参导致错页；现有 16-page group 结构不变。32 指账户范围，不意味着单页订单行数有固定上限，也不构成订单配额。

只重算失效页、共享 projection 目录，以及失效页合计 ≥512 行才交给 Rayon，均属于当前局部性能实现。单个账户的所有订单仍位于同页，因此极端集中输入的单页计算仍顺序执行；不能据此声称所有输入都能充分多核并行。`source.pages()` 和 layout 对比仍扫描页目录，目录 COW 更新可能复制目录，故也不能宣称已实现 ADR-0018 的完整增量状态根或与全部活跃规模无关的 tick 成本。这些是诚实的性能边界，不是本批逻辑缺陷。

## 3. 缓存、错误与跨层边界

### 页身份与旧快照

- 缓存保存叶 `Arc<BTreeMap<EnvelopeKey, Envelope>>` 的 `Weak`，不是仅保存裸地址。Weak 持续占有旧 allocation 的控制块，阻止旧地址被回收复用造成 ABA。没有 `unsafe` 或绕过 COW 的可变入口。
- LedgerPagedMap 的 `get_mut`、`insert`、`remove`，以及测试专用 `values_mut/retain` 都经过叶 `Arc::make_mut`。有多个 strong owner 时 clone 新页；只有一个 strong owner 但存在 Weak 时 dissociate Weak。因此任何可能改变叶值的入口均使已缓存身份失效。
- clone ledger 可以共享 cache，但 projection 时逐页重新匹配该 ledger 实际拥有的叶身份；旧、新 ledger 交替访问只能影响复用效率，不会读取另一份 ledger 的金额。
- terminal removal 删除 live 页或改变叶身份；reset 清空目录后 layout 检查删除 projection；rebase 实際变更通过 `get_mut` 失效，未改变 live 的快速分支可以合法复用。terminal、audit、conservation 独立变更且不影响 live 时不必使资源投影失效。
- projection 目录由 `Arc::make_mut` 更新。已交付 LiveReservations 持有旧目录 strong 引用，旧目录中的 `Arc<ReservationPage>` 只读，因此旧 seal 快照不会被重写。

### checked 汇总与首错

`EnvelopeKey::Ord` 先比较 account，LedgerPagedMap 的 group/page 组合按 account 范围升序；`pages()` 顺序与旧完整 `iter()` 一致，同账户不跨页。页内仍按 key 顺序先执行 cash 的 checked add，再执行股份 checked add。并行分支由有序 `Vec` 的 indexed parallel iterator 收集 `Vec<Result<…>>`，再顺序收集 Result，所以返回最早 page 内最早 key 的原有错误，完成顺序不会决定首错。未变化的缓存页此前已成功汇总，不可能隐藏新溢出。没有跨账户股份总量检查或新增整局 shares 溢出约束。

所有可失败 projection 在 `live_reservations.rs:179` 之前完成，失败时不会进入目录 retain/extend，先前成功目录完整保留。锁毒化以带 location 的 StepFatal 报告，不静默 fallback。

### AccountBook 的完整依赖

ResourceValidationCache 隶属于不可变 AccountPage；AccountPage clone 清空 cache，账户 `get_mut`、`values_mut`、insert 等入口继续 invalidate。其另外两个依赖分别为完整 prices 值和此账户范围的完整 ReservationPage 值。projection identity 相同可直接命中；identity 不同但实际预留内容相同也可合法复用。None/Some 差异不能命中。保留零值 account/stock 行意味着未知引用没有因缓存丢失。

生产调用只有 `DecisionResourceSnapshot::seal`，该 closure 读取的账户余额、持仓、T+1、成本均在 AccountPage 内，报价在 prices 内，预留在 ReservationPage 内。没有额外未纳入 cache 的业务输入。AccountBook 仅在该页全部账户验证成功后安装 cache；失败不安装失败输入。其他页可缓存成功结果，但全局未知引用检查仍再次运行，这不改变失败原子性。

原 AccountBook 已使用 Rayon `try_for_each`，同时多个账户页损坏时不保证某个固定账户错误最先返回；本批没有新增这一边界。新增的逐 envelope 汇总首错则保持旧 canonical fold 的稳定顺序，二者不能混为一个承诺。

### serde、hash 与 Eq

EnvelopeLedger 的新增 cache 明确 `serde(skip)`；手写 PartialEq 和 `EnvelopeLedgerHashProjection` 均没有 cache，session hash 仍只消费该 projection。构造新 ledger 初始化空 cache。此类型原本只 derive Serialize，没有新增 Deserialize 路径或存档恢复要求。LiveReservations 的 Eq 只比较 page/projection 的账户预留值，CachedPage 手写 Eq 忽略 Weak 身份；DecisionResourceSnapshot 不因不同 allocation 身份而出现业务不等价。

## 测试证据与非阻塞补强

静态阅读看到新增五个测试覆盖：未变化页复用、单页 insert 失效和旧投影不变；canonical fold、u64::MAX account、零 shares stock；现金/股份 overflow 与真实 ≥512 行门槛的 Rayon 1/4 首错；不同账户股份不混加；apply/release、terminal removal、clone 旧快照、同页再插入无 ghost；未知 stock cache 命中继续拒绝与未知 account 优先。AccountBook 原有测试修改输入表示，保留 mutation、价格、现金/股份预留与旧 authority 的断言，未发现弱化断言。

以下是专项回归覆盖补强，**不是已复现的实现缺陷，也不阻塞当前生产逻辑静态结论**：

1. 暖 projection 后仅有唯一 strong owner 的 leaf `get_mut`，验证 Weak identity 失效；暖 cache 后 rebase/reset，验证数据与旧 snapshot，含 rebase 不改变 live 的情况。
2. 先暖成功投影，再同时删除页、改变页并引入 overflow；失败后修正输入，验证失败没有部分安装目录，旧投影保持不变。
3. cache 预热前后 EnvelopeLedger Eq、Serialize 与 hash projection 完全相同；不同独立 allocation 的相同 LiveReservations 及 DecisionResourceSnapshot 值相等。
4. AccountBook 相同失败输入连续调用仍执行并报错，成功旧输入复用仍有效；None→Some→None 和同值不同 projection identity 的边界。

root 已表示将添加这些短断言，并另开新 reviewer 检查新增测试。本 reviewer 不参与实现、不会复用作该后续批次 reviewer。root 报告最新短测 live5/account_book5/decision_resources11/ledger56/continuous101/NPC21 通过，命令 9s deadline、最长 7.21s；这是 root 提供的执行证据，本 reviewer 没有独立运行测试，不将 filter 计数相加为唯一 case 总量。

## 性能及诊断证据边界

`substage-profile-findings.md` 明确 26 个临时 span 只用于定位，父子及跨股票 span 可重叠，不能加总为 tick wall time 或当作正式吞吐；临时 module/script/patch 归档是诊断材料，不是本批生产接线。当前生产 module 未保留 performance_profile 接线。撤回逐字节证据由 root 留存，不在本次重新执行恢复验证。

root 提供已无构建重叠的相邻测量：base 127.85x，candidate 139.66x/142.36x。该数值支持当前机器、相同负载下的局部收益，不证明 ≥1000x、长期稳态或三宿主验收。此前有构建重叠、读取旧 artifact 的 sparse 样本已标无效，不能用其结果替代最新实现证据。frontend 继续暂停，既有 save_contract 日历问题及其他未验收范围仍需如实报告。

## 本次静态版本指纹

| 文件 | SHA256 |
| --- | --- |
| account_book.rs | `55728101c3fa90e2e90fb2cb339ba6cd0a58bae3909781f8678ccc0adb5bab0d` |
| decision_resources.rs | `3da3a3a641fdd763a3a632ca594ebeed836369f67bdffb20ffb49c2911702cd2` |
| ledger.rs | `ff2371954047fff6a52f45fdc5e7496c47fa0a73b5b80cd60bb99e7db2a0bbd6` |
| ledger_paged_map.rs | `bb61593b6b5b26df6a98ada53a87dbc76e23233ee62b145bd1c547e88592a4dc` |
| live_reservations.rs | `d41abad34299ed1f81ad2ff075d310af743bc4352a8d5bb1872622725a318e32` |
| mod.rs | `98c1ded32d7e616e06bedc7af3a4634b80102e860a75834c3785084c338ac252` |
