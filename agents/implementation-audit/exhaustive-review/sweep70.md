# Sweep70：Pipeline 三份独立 review 的残余承诺

本轮只读审计与新增本记录，未修改产品、Git 状态历史或测试，未运行长验收。源码工作树沿用主控指定的 merge 等价基线；独立 review 原 `b89afb3` 绑定只证明该历史审查范围，不自动证明当前整套 runtime。

## 全文覆盖

| 文档 | 行数 | 全文读取 |
|---|---:|---|
| `agents/oop-refactor-implementation/pipeline/review-auction.md` | 80 | 1–80，含最终新增源码/SHA gate、结论、规则、动作、失败、测试限制 |
| `agents/oop-refactor-implementation/pipeline/review-continuous-core.md` | 78 | 1–78，含唯一已关闭发现、17文件清单、fixture 收口、manifest |
| `agents/oop-refactor-implementation/pipeline/review-resources-projections.md` | 51 | 1–51，含七条结论、N07 参数化建议和最终证据 |

共 209 行，连续工具输出未截断。已按本轮根 AGENTS/principles 与现行 ADR-0017/0018、trading-rules 解释历史动作，不恢复旧来源类排序、sealed ID 优先或 tick 内 P3 预算回补。

## Auction 逐章核对

| 原文章节/条款 | 当前 caller / 源码证据 | 状态与反证 |
|---|---|---|
| 范围/限制 :3–9 | `auction_tick_transaction.rs:230` capture boundary；`stock_stream.rs:182` finish boundary 入参 | 生产接线存在，原说明是静态 review，不能写成本轮 runtime 通过。 |
| 新测试覆盖 :11；六文件内容绑定 :21 | `auction_refactor_tests.rs:16/:31/:52/:100/:132/:161/:209/:269/:297` 九个实际测试仍在 | SHA/line count 是内容绑定，不是执行证明。本轮直接定位实际测试，不以历史未跟踪标签推断当前未交付。 |
| 结论 :38 | 当前相同 receiver 与 finalizer 主干仍在 | 没有未关闭有效问题，不将「未执行运行」转为代码遗漏。 |
| A股依据 :42；supplied顺序 :48 | `auction_day_end.rs:486` 逐输入操作执行；`:1602` 开盘余单按 arrival_seq；`:406` results 股票排序在 worker 完成后 | 输出/首错排序不等于交易优先。当前代码反证，未发现恢复旧来源顺序。 |
| 竞价拒单/日终 :50–52 | `auction_tick_transaction.rs:230` 阶段边界；`auction_day_end.rs:1178` 共同日终；`continuous_tick_finalizer.rs:223` 同一 transition | 两条 production 路径均有调用；T+1 在共同 transition，未发现漏迁移旧自由函数入口。 |
| A02/A03/N02/R2-N02必要性 :54–63 | `AuctionTickBoundary` 私有 fields `auction_day_end.rs:940`；capture `:947`；TradingDayEndTransition `:2127/:2140` | 拥有 shadow 的 receiver 与短生命周期 candidate transition 已实现，不要求新增长期 manager/权威副本。 |
| 错误与部分写入 :65–72 | boundary `:959–982` checked tick/phase/day；day-end `:2144–2154` live拒绝，`:2156` unlock，`:2188` take plans，`:2167` candle，`:2169` day溢出 | 原文明确允许 private candidate 局部部分进度，不承诺局部 rollback。外层 P9 `candidate_commit.rs:46/:97–98` 隔离并最终 swap，故不能把 unlock-before-plan-failure 误记为 authority 原子性漏洞。 |
| 测试限制/后续门禁 :74–80 | 上述九测试与 production 两 caller 有源码；主任务运行证据由对应验证记录负责 | 需运行验收这一句是证据承诺，未在本轮执行。跨phase直接调用 transition 测试不替代完整生产旅程。 |

## Continuous core 逐章核对

| 原文章节/条款 | 当前证据 | 状态与反证 |
|---|---|---|
| 范围/结论 :3–11 | 相关 owners 与当前主干 | 此 review 仅静态范围，未承诺整个并发架构全部 runtime 已完成。 |
| 唯一 caller 遗漏 :13–19 | `pipeline/mod.rs:252–253` 两处 clear 现在都在 `game.state` | 已关闭发现有当前精确代码反证；不复活旧不存在字段编译错误。Position fixture 仅适配构造器，未发现新增语义承诺。 |
| A股/输入序 :21–25 | `continuous_matching.rs:344–345` supplied operations 顺次执行；`incremental_continuous_stock_shadow.rs:291` worker 结果排序 | 处理后汇总排序不引入委托优先级。按现行实际并发受理契约解释。 |
| consuming失败 :26 | `incremental_continuous_stock_shadow.rs:187` 标记 failed；`adaptive_plan_chain.rs:315/:330/:890` 失败状态 | receiver 不提供局部 retry，整 tick 候选丢弃符合当前 contract；不存在要求恢复已消费 shadow 的未实现功能。 |
| receipt/fact消费 :27–28 | `continuous_matching.rs:1077` FillReceiptProjection；`adaptive_plan_chain.rs:55/:73/:696/:861` contains/commit_round | 事实消费集合在成功后推进，finalizer只查询，不重新 Settlement。 |
| lifecycle诊断 :29 | `continuous_lifecycle_projection.rs` receiver 实际存在；candidate finalizer保留原事实 | 临时跨订单输出不定义交易时间。原审查没有承诺解决诊断订单关联 G37。 |
| DayEnd :30 | `continuous_tick_finalizer.rs:223` 共同 transition；`auction_day_end.rs:2156–2173` T+1/清理/日K/day | 局部写入失败由候选隔离；不需要额外内部事务补丁。 |
| NPC lifecycle :31 | `session.rs:2378` ensure_order_absent 实际 caller；`quote_expiry.rs:33` owner方法 | duplicate检查不是悬空 helper，已注册 producer caller。其既有 panic 不因 review OOP 范围说明自动成为新公开 API 缺口。 |
| P0/P1 :32 | `quote_expiry.rs:78–84` checked累计后append；`:90` 禁止重复expiry；`decision_resources.rs:112–136` postP0 live扣资源 | 不额外加 aggregate releases；SelfView replaceable不进入P1预算。 |
| 必要性/测试 :34–40 | consuming、fact、release、lifecycle边界实际生产方法与测试在对应模块 | 没有未关闭后续功能；短fixture覆盖只能说明源码存在。 |
| 文件绑定 :42；fixture再次复核 :68；最终manifest :76 | 独立范围与最后内容绑定声明 | 未重新签署其SHA，也未冒认其测试。历史fixture literal构造替换不增产品义务。 |

## Resources/projections 七条结论与残余

| 原文条款 | 当前源码 / caller | 状态与限度 |
|---|---|---|
| 范围/依据/结论 :9–25 | 原列模块仍存在，生产权威入口 `session/failure.rs:73` | OOP静态复核通过不能核销整模块所有旧功能缺口。 |
| 1 AccountBudget :27 | `account_validation.rs:1116–1122` snapshot读现金；`:1148/:1152` cash/share lane | Money与u32股数分离，未发现读P4释放回补。 |
| 2 seller fee cap :28 | `transition.rs:197–229` unpaid/cap/commission→stamp→transfer；`ledger_validation.rs:291–317` 独立复算 | 游戏费用封顶明确不是交易所真实规则；独立validator未换成producer共享输出。 |
| 3 Settlement首错 :29 | `settlement.rs:151–176` gross/commission/stamp/transfer/qty checked顺序 | 已有真实执行路径，未新增跨层费用重复计算。 |
| 4 AccountFillProjection :30 | `retail_projection.rs:405–418` 全job Result先检查；`:422` final_position错误后；`:624` finish仅存错误 | 没有把早账户final错误提前覆盖较晚账户Fill错误。watchlist修剪`:634–639`。 |
| 5 InstitutionalFacts(moment) :31 | `retail_projection.rs:332/:338/:529/:597` dated机构路径 | 仅机构有 dated writer；`:519` Retail仍 record_fill_with_order，不能据本条核销散户日期/20日衰减 G08。 |
| 6 experience capture :32 | `decision_snapshot_capture.rs:400` SelfView，`:401–417` risk；`candidate_commit.rs:59` queue下一tick NPC | 拍摄caller在提交前candidate，错误不会提前安装authority经验。 |
| 7 SelfView现金 :33 | `decision_snapshot_capture.rs:436–445` raw−reserved+replaceable；`decision_resources.rs:136` P1仅raw−live | 观察报价与执行预算有不同职责；这不是「遗漏将replaceable资金加入P1」的代码缺陷。 |
| 测试限制 :35–43；N07统一参数化 :41 | `transition.rs:298/:333` cap边界及overflow；`transition_tests.rs:121` 分项优先；`ledger_validation.rs:122–125` 无movement Fill拒绝 | 原文明确统一全分项参数化为可补证据、非算法迁移阻断。未有新测试直接等同于功能漏实现。局部Settlement零量skip不等于生产ledger允许零movement Fill。 |
| SHA证据 :45–51 | 原manifest/diff仅绑定历史范围 | 未声称重新运行、重复签认或借其静态SHA证明长验收通过。 |

## 新候选与反证

本批未找到需要新增 G 编号的确定代码遗漏。唯一原有效发现 `plan_tick` 两个 clear caller 已修，当前代码证实；N07 producer/validator 全分项统一参数化仍是明确可补测试证据，不应包装为交易功能缺失。

三个review共同的“主实现者仍需运行测试/编译”属于运行验收证据，主任务后续验证记录应负责；本轮不重新执行，也不因旧审查没跑就推断当前构建失败。大 A 部分沿现行正式规则及游戏简化，不恢复旧sealed身份排序、不把局部candidate部分写入误报成authority半提交。散户dated/衰减 G08、诊断关联 G37等既有总账缺口并未由本轮OOP提取修复，继续保留。
