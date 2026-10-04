# NPC 与连续撮合回归修复

## 根因与修复范围

完整回归收集到的 NPC 请求断言失败来自旧 fixture 仅把 `ZiNoiseStrategy.arrival_rate`
设为1，却没有定义后来接入的本人 `AnalysisProfile`。现行 personal analysis 可合法选择
`Watch`，到达概率为1不保证下单。需要买单的短场景显式选择本人 price-volume-only
`AnalysisProfile`，以已有玩家现金支持的900分、100股非交叉被动买单形成正盘口
imbalance。连续阶段通过已有 `seed_order_for_test` 建立合法测试簿与 envelope，
该 helper 不代表执行了完整的玩家请求和 `AccountValidation` 链；PreOpen 场景先通过正常请求进入
opening auction，再完成599→600的正式竞价结算与未成交委托转连续簿，绝不在
禁止申报的 PreOpen 塞单。helper 明确检查 best bid/ask；不造股份、成交或个人
策略输出，不改前史、开局1000分价格、日内分钟样本或生产策略。

`price_volume_signal` 的两个原子可分别不可用：缺少30分钟窗口时，量价原子仍
诚实不可用，但真实 imbalance 原子有值可以参与评分；没有为了测试改变生产的
历史不足语义。

空来源与后续非空来源的投影场景明确将第一个账户到达概率设为0、第二个设为1，
不再依赖旧 seed 恰好产生空决策。原始来源 key、数量、不路由、策略状态转移、
next-tick 真实现金（含费用）的断言均保留。

两项 `Experience(NoActiveEntry)` 及三项 executor perturbation 失败来自测试直接
`grant_position` 后没有登记初始
holding epoch。补用 `initialize_holding_dated` 登记已有股份的生命周期与所见价格，
不是虚构买入；仍保留被动成交清除生命周期、分批收据只结算一次、真实费用与
T+1 的断言。生产策略、撮合顺序、存档契约、Money 编码均不修改。

## 验证

修改前13项短失败已经在 `.tmp/main-regression-2026-10-05/rust-case-output.log`
记录。首轮 EarningsMultiple-only 加低报价方案实际短测2绿8红，不能保证
synthetic 公司公开盈利可用，已撤销该未验证假设。第二轮虚拟 trend-only 方案
13项短测6绿7红，已撤销：正式 `build_price_path_observation` 在没有完成分钟
时不提供包括 five-day 在内的 return，这个正确的生产边界保持不变。
共享工作区由主任务统一 Cargo 构建。最新 imbalance 方案实际13/13短测通过，
每项 `--exact`，外部10000ms deadline，最多8个进程同时运行，每个进程
`RAYON_NUM_THREADS=2`、`--test-threads=2`；最慢 executor 多预算用例为7.39秒。
编译来源为 `.tmp/main-regression-2026-10-05/repair-npc-second-build.jsonl` 中实际
Cargo artifact，日志为同目录 `npc-imbalance-0.log` 至 `npc-imbalance-12.log`。

非作者 `npc_fixture_review` 已完整读取7个文件并检查最新完整 diff：A股禁入时段、
真实现金支持、T+1、来源身份、生命周期与原有效断言均保持，不改生产策略；记录中
continuous 测试 helper 与正式受理链的区别已按其发现修正。完整回归由主任务统一
执行，本记录不冒称完整回归已经通过。
