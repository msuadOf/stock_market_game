# Session OOP 实施与交接

基线：`b89afb3346743a4b4fccf26c9ac9ff108595f696`；权威需求是 [完整 action-index](../../oop-refactor-audit/challenge-2026-10-03/action-index.md) 和 [relationships](../../oop-refactor-audit/challenge-2026-10-03/relationships.md)，历史 JSON 的旧 50 动作正文不覆盖完整方案。

19 项主动作及 `session-R2-E01` PublicationFactCursor 增强均已落盘，包括所有标为可选/探索性的子目标。逐项真实 owner、方法定义行、生产 caller、需求核销说明、短测 filter 和复核链接见 [action-ledger.json](action-ledger.json)。完整所属源码及新增文件内容 SHA 见 [source-manifest.json](source-manifest.json)；该文件排除另组持有的 pipeline 目录。

## 已保留的边界

- GameSession 只有一个 CommittableSessionState；poison 和测试失败 hooks 留在 facade。clone 的 AccountBook 校验、commit 原三组分页并行释放、tick 外 pending_player 与自然日时钟更新保持原路径；没有 Deref/DerefMut 访问状态。
- SessionCandleBook 内拥有完整 history 和 active；SaveSlot、Snapshot 和 hash 继续投影原独立双字段，不截断完整历史、不迁移 schema。
- 四种机构个人状态合为同账户 BeliefParticipantState。按 root 裁定使用 AccountPagedMap 维持已有 COW，attention 仍独立覆盖全 NPC。SaveSlot 仍展开为四张旧 map，hash 字段和键顺序保持。重复 install 保留 attention 首错与 watchlist 新值已写、其余三成员旧值的原失败残态。
- DecisionChainObservation 与 RootReadContext 仅读一次封存事实。InstitutionDecisionRoot 只持单账户个人工作，不路由、不分配订单游标、不生成 PlanLifecycleAction。反馈后的 PlanLifecycleReview 从现行 candidate PlanBook 重新复核，固定资源观察不被当前 live 账户替换。
- PlanRootCoordinator 只协调个人工作与 typed 增项交还。StockRouteCoordination 保留三个可独立消费的上下文 map，generation、retry 与 reconsideration 原次序不变。
- TradingPlan 与 ParentOrderPlan 外部读取走 getter，真实生命周期写入 receiver。DTO、RNG 抽样、Money 分/qty 股、T+1、零股余量、价格笼子、费用累计及日终持久化门禁不变。可选快照预留、fee audit、target proposal、RetailDecisionContext 只收拢既有纯派生事实，不新增交易政策。

交易制度没有新增或修改；领域依据沿用仓库已有 ADR 和 trading-rules 的官方来源、适用日期。本轮没有重新访问交易所官网，不能把旧登记日期宣传成新的官方核验。

## 独立复核

- [核心 owner](core-review.md)：唯一 state、个人状态移动/重复失败残态、SaveSlot/hash 顺序；有效发现是 N04 代表性覆盖缺口，已补四成员隔离、独立存档投影及 duplicate 残态测试并复核。
- [leaf](leaf-review.md)：计划/策略、OpeningFigures、恢复校验、收费 audit、快照汇总；有效发现为 getter/poison caller 漏迁与 ZiNoise 全参数投影测试缺口，均修复再次复核。
- [生命周期](lifecycle-review.md)：母单、ProtocolState/PublicationFactCursor、K 线与注意力；缺失私有 paired writer、旧 test getter 及 unlinked 反向替换测试缺口，已修复再次复核。
- [额外旧 caller](caller-review.md)：10 个旧 caller/test 的完整 diff，51 个旧 case 和190 个 assertion 保持，非法 strategy、i64::MAX 价格以及 poison/hash/save 行为守卫没有弱化。
- [root/续作](roots-review.md)：完整新文件和原主体/caller；Coordinator 多余 append 与 RootReadContext 重复新增校验均已删除，保原失败/观察语义，再次复核关闭。

## 实际验证边界

各实施 worker 按 root 的集中验证指令，没有运行 Cargo 或产品测试。保护行为测试在实现前或迁移过程中加入；没有执行实际 TDD 红灯，不能把未定义方法导致的编译错误当作行为红灯。精准 rustfmt 和限定 diff whitespace 检查已执行。root 已集中执行编译 01–04，发现的 getter、可见性与 fixture 接线错误均已逐项修复；集中编译 05 已由 root 报告 engine 和三宿主 testlib 通过；all-targets 类型检查 06 与精确 lib 短 case 正在由 root 执行，以 validation 目录实际结果为准。

63 个所属源码（含 2 个新增 owner 文件）完整 diff 已由未实施者独立三门静态复核覆盖，所有有效发现修复后再次复核。

此记录现在同时包含源码实施、独立静态复核和代表性短测/编译的实际执行证据；这些证据不表示运行了完整回归。没有执行长期模拟、完整浏览器矩阵、Git 写入、push 或发布。

## 最终集中运行证据

root 执行 [rust-lib-short-01-result.json](../validation/rust-lib-short-01-result.json)：232/232 个计划逐 case 短测通过，覆盖本组 19 项当时计划的代表性 case。母单未关联计划反向替换增补在 [rust-short-increment-02-result.json](../validation/rust-short-increment-02-result.json) 通过；该批次共 5 个追加 owner case（另 4 个为跨组 N07）及36 个最小 integration case，首次39/41通过。两项 WS listener fixture 因沙箱 EPERM 失败，保留原断言在沙箱外重跑后 [ws-fixture-short-final-result.json](../validation/ws-fixture-short-final-result.json) 2/2通过；没有把初次环境失败改写为通过。短测使用8个进程并行、每进程 Rayon 4，单 case 与整命令均为10000ms硬上限。各项精确 case id/filter/结果链接见逐项台账。

成熟最终源码下 [targeted-tests-build-07-result.json](../validation/targeted-tests-build-07-result.json) 的 engine/server lib 及16个 integration target 编译通过；[rust-all-targets-check-08-result.json](../validation/rust-all-targets-check-08-result.json) 的四个 package 在诊断/host features 下 all-targets 类型检查通过。更新证据时 [rust-default-targets-check-09-result.json](../validation/rust-default-targets-check-09-result.json) 也已写入 exit_code=0，四个 package 默认 features 的 all-targets 类型检查通过。上述构建检查不表示运行了所有 target，不是完整回归。源码没有因本次证据更新而改动。

## 最后无用入口清理

root 在最终 warning 核查后批准仅删除 `decision_chain.rs` 的 `run_chain_for_account` 迁移过渡包装器及属性：基线1个生产caller和2个test caller已全部迁入 InstitutionDecisionRoot，删除前全仓只剩定义，即使 `cfg(test)` 也无使用。符合 ES03 的“无剩余调用点后移除包装器”要求。真实root工作入口和所有测试断言不变，其他源码保持freeze。原roots reviewer已对该文件完整baseline diff再次独立复核通过，并以重插原29行后匹配前次签署SHA证明其余字节不变；该文件的本组source-manifest/action-ledger SHA及方法定义行已更新。root已重编受影响testlib并通过，精确执行原计划source=decision_chain.rs的2个case均通过（0.6011s/1.778s），诊断/host features与default features四包all-targets检查也全部通过且零warning；执行证据已补入台账。没有自行运行或扩大回归。

末次清理最终结果：[engine-final-build-10-result.json](../validation/engine-final-build-10-result.json)、[decision-chain-short-final-result.json](../validation/decision-chain-short-final-result.json)、[rust-final-features-check-11-result.json](../validation/rust-final-features-check-11-result.json)、[rust-final-default-check-12-result.json](../validation/rust-final-default-check-12-result.json)。同源2个case的新执行绑定 `b6696ad873d44ab39443ea99d21f31e1dd26e658ad447e494f3971da644399d5`，替换旧case执行记录；[final-validation.json](../validation/final-validation.json) 的278个唯一case计数不增加。本组19项实施、独立复核和代表性运行门禁均已完成；未执行完整回归。
