# 因果诊断两项既存失败修复

## 根因与范围

基线 `0c6a779`。两项失败共用 `npc_execution_session`：原285分场景产生272张
买单、没有卖单，成交前置条件不成立，因而没有 Execution、Filled 或 impact。
诊断不应在无成交时伪造这些事实，生产市场也不能保证 NPC 自然产生对手盘。
原 `npc_execution_reconciles_every_share_and_preserves_provenance` 与
`real_fill_stream_rejects_duplicate_fill_and_wrong_execution_price` 的断言全部保留。

新增真实双边成交前置测试，在旧场景下失败，日志 `.tmp/causal-test-fix/red.log`。
修复仅调整此短诊断 fixture：选用600分价格场景使 NPC 自然生成卖单，每个交易日
通过公共玩家入口申报100股 Highest 买单。股份仍由初始化分配给 NPC，玩家只用
自己的既有现金；不修改账户、股票、策略或诊断生产代码，不注资、不手造 Trade，
不跳过笼子、费用、股份校验或 T+1。Highest 仍依 ADR-0022 在真实受理时解析。

fixture 同时核对真实 Trade 的数量、不同账户对手方、玩家买入当日 T+1 锁定，
成交量对应诊断 Filled 双边数量，交易前后股份总量守恒、投资者现金因费用减少。
已有来源、计划、个人获知、撤单与日终余单守恒，以及重复 Filled 和篡改执行价
的具体拒绝断言没有弱化。

## 短验证与独立复核

构建 `cargo test -p engine --features simulation-diagnostics --test causal_diagnostics
--no-run -j32`，使用进程外300000ms编译上限。二进制来源记录于
`.tmp/causal-test-fix/build.jsonl`。三项相关 case 独立并发，分别使用十秒进程树
deadline，通过，约2.7秒；不是长模拟或完整回归。

最终14项因果诊断套件全部通过，日志 `.tmp/causal-test-fix/final-suite.log`，
最终源码重编译后约7.06秒，外部10000ms deadline、`--test-threads=16`、每进程 Rayon 4 worker。
完整回归未运行。非作者 `review_causal_test_fix` 完整复核基线 `0c6a779` 至本批
测试与三份工作文档差异，结论通过，无阻断发现：一手100股、公共限价选价、既有
股份与资金、T+1及扣费符合现行游戏A股契约，没有修改撮合规则；原断言逐项保留，
新增守恒前置校验不伪造流动性，也不宣称任意自然NPC场景保证成交。改动限于诊断
fixture及相应记录，不混入尚未讨论的策略选择。复核者独立执行实际构建二进制的
两项 `npc_execution` 短测及 `real_fill_stream_rejects_duplicate_fill_and_wrong_execution_price`，
分别2.04秒及1.98秒通过，均使用外部10000ms deadline、16测试线程、4 Rayon worker。
额外10000股试验异常证据与未修复边界如实保留，不将其报告为生产代码已无缺陷。

## 未混入的额外试验发现

曾用每日10000股玩家买单扩大成交，三路同场景测试中一项在散户分析阶段拒绝
`-157`分净成本，另外两项通过；日志
`.tmp/causal-test-fix/real_fill_stream_rejects_duplicate_fill_and_wrong_execution_price.log`。
这是大成交量下的另一条分析边界，不是原无对手盘失败，也不是重复收据或执行价
校验失败。本次使用代表性一手 fixture，不宣称修复该额外问题；保留证据供单独
定位，不能删除异常、吞错或禁止合法盈利交易来绕过它。
