# PrivateStockLedger 独立复核（2026-10-06）

通过本批独立复核，未发现必须修复的实质问题。本 reviewer 未参与实现，没有修改实现。

复核范围为 `.worktree/engine-max-performance` 相对 `HEAD 5bc20409` 的完整本批 tracked diff、新增 `packages/engine/src/session/pipeline/private_stock_ledger.rs` 和 `private-stock-ledger-findings.md`。已读取 AGENTS.md、principles、trading-rules、ADR-0017 现行阶段/收据/失败合同、ADR-0018 §7、open-questions 和 architecture。历史 untracked 日志、图片、旧报告及已撤回的数学 Copy helper / page proof 实验不属于本批实现范围。

## 1. 大 A 语义与依据

保持既有大 A 语义及已登记的游戏简化，没有新增交易制度或改变费用参数。依据仍为 trading-rules 登记的沪深现行规则（2026 版自 2026-07-06 施行）、既有价格时间优先核对记录及 ADR-0017/0018。本次未重新访问官方资料，也不把中国结算费用资料访问失败边界改写为当日重新核验通过。

`continuous_matching.rs:286` 仍先校验 initial snapshots，再于 `310` 移动已验证 Envelope，累计 filled quantity/value、nominal/charged fee audit 均保留。撮合、symbolic price resolution、价格笼子、T+1、同 tick 释放不回补、资金/股份预算与日界逻辑未修改。Money 仍为分，shares 仍为股。权威 EnvelopeLedger、公共存档、hash/serde、Settlement 与宿主契约保持原形。

`private_stock_ledger.rs:145` 复用原 normalization、ledger_validation::validate/next_audit 和 Envelope::apply；卖单 cash envelope 为零、佣金→印花税→过户费优先级与历史欠费追收仍由同一 helper 校验。canonical 排序只用于账本查询、检查与输出，不赋予请求新交易优先级。

## 2. 必要性与最小范围

改动针对已记录的逐股私有账本每 tick 重建热点，将同键 Envelope、ConservationState 与 terminal 标记放入 PrivateRow；tick 起点 Vec 排序一次，同 tick 新建行使用小 BTreeMap。范围局限于 Continuous StockProcessing 私有状态，没有新增依赖、公开 API 或另一套交易规则。

`ledger_conservation.rs:27` 的 record_state 提取原 checked 运算；`42` 的 validate_owned_rows 复用原 row 方程、map_rows 和 checked totals。一般 ledger 校验未弱化。LiveEnvelopeLookup 让私有 ledger 与测试用一般 ledger 共用原 fill projection，复杂度与本次替换需要相称。

probe 在 wall-clock 计时停止后读取 final_load，读取失败显式返回错误，不跳过计算或减少负载。工作记录没有把撤回实验、CPU 配置或短期观测写成全部验收完成。

已有相同默认 5 股、20007 NPC、seed42、16 物理核 affinity、Rayon16/Tokio2 的真实 server 记录支持保留此优化：原约103.2–103.8x，Vec 首测128.45x；root 在复核结束前补充最终复测127.50x。本 reviewer 未重跑性能测试，不把这些数字当作满核、稳态、全交易日或 >=1000x 达标证明。

## 3. 首错、边界与失败隔离

未发现阻塞本批的遗漏边界、跨层漂移或不必要复杂度，重点核查结果如下：

- `private_stock_ledger.rs:186` 的 evidence 保留全部 live canonical 行→全部 terminal canonical 行的顺序；先守恒，再检查 live/terminal 资源形状，最后 Envelope::validate。同键 audit/conservation 共同拥有，不再存在独立 map 丢行、audit 漂移或 live/terminal 重叠的可构造路径，省去对应结构检查合理。
- `ledger_conservation.rs:45` 并行计算行结果后，按原顺序读取 Result 并 checked 累计；较早 aggregate cash/share overflow 仍先于后续行守恒错误。新对照测试锁定完整描述、location 与第三行错误优先级。
- normalization 的 receipt index overflow、envelope/local key 一致性、来源内连续 ordinal 和 batch duplicates 来自原 helper；seen_local_keys 跨轮保留并在 audit 查询前检查，跨轮重复不放宽。terminal 后的 receipt 仍先做原 kind/audit 校验，再返回原 unknown live envelope 错误。
- `private_stock_ledger.rs:104` 先准备完整批次后安装；批次重复与 terminal key 冲突不安装前缀。terminal 行保留且禁止复用 key，get/iter 只公开 live。initial Vec 与 created BTreeMap 按完整 EnvelopeKey 两路 merge；key 无可变入口，转移不会破坏 binary search 或 canonical 顺序。
- 私有 apply 在后续错误前可能修改局部 candidate，与原 apply_private_for_stock_round 的合同相同，不承诺局部 rollback。processor 消费 owned ledger，错误不交出输出；coordinator 标记失败并禁止继续/finish，最终 ReceiptAggregation/CommitTick 仍核对权威资源守恒。失败不会形成部分 tick 提交或外发事件。

本次专属短测已覆盖累计费用、拒绝输入、canonical merge、重复/terminal key 与 cash/share 首错；>=512 行和混合 terminal 首错由一般 ledger/shared helper 既有测试覆盖。私有大型混合行的直接对照可增加覆盖，但本次未发现新分支使它成为阻塞修复。

## 验证边界

任务提供的最新 binary（mtime 晚于本批源码）定向验证通过 PrivateStockLedger6、Continuous101、ledger56、envelope9、stock_stream15；每条命令 timeout9s、4 test threads，未运行 Cargo 构建、基准或浏览器。首次误用宽 continuous filter 命中133项并在9s以exit124终止，未记为通过；随后收窄本批 filter，101项在7.30s通过。两个错误的 ledger 路径 filter 命中0项，不计通过数量。filter 可能重叠，不相加为唯一 case 总数。

本批门禁通过不代表 WASM、HTTP/WS、完整 workspace、全交易日、前端或整个性能目标完成；既有 save_contract 日历错误契约失败按实现记录仍未解决。>=1000x 目标继续在途，前端保持暂停。
