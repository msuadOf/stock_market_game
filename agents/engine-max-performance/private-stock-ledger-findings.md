# PrivateStockLedger 优化记录（2026-10-06）

## 范围与依据

只优化真实 Rust server 复用的 Continuous StockProcessing 私有账本。新类型共同保存 Envelope（包含累计 audit）、ConservationState 与 terminal 标记，tick 初始行以 Vec 排序一次、同 tick 新行以小 BTreeMap 保存，完整 EnvelopeKey 按两路 merge 输出。输入 snapshot 校验后移动 Envelope，不再次 clone。权威 EnvelopeLedger、存档/hash/serde 契约与所有交易规则保持原形；不新增交易制度假设，语义依据仍为 docs/trading-rules.md、ADR-0017/0018。

receipt normalization、来源/费用/audit/资源检查、Envelope::apply 与守恒 helper 均复用原逻辑。完整检查保留先 live 后 terminal 的行序、原顺序 checked totals 和首错合同。失败时外层丢弃整个股票/tick candidate，私有转换不新增局部回滚承诺。terminal 行保留，禁止复用其 key。

## 测量

同默认 5 股、20007 NPC、seed 42，物理核 affinity 0–15、Rayon16、Tokio2，真实 SessionActor Fastest 广播；每一帧检查连续 tick，不跳过计算与休市自然日。

- 已独立审查并提交的 HEAD 5bc20409：稳定约103.2–103.8x。
- 单 BTreeMap 私有行：122.462x。
- Vec + 新行 BTreeMap + 移动 snapshot：128.454x，3001 tick /23.362379s；actor 最近窗口约84.19x。CPU 样本均值3.99核，16 核预算24.92%。不称满核、稳态或完整交易日达标。
- 纯 engine 辅助阶段诊断136.31x：decision/coordinator7.11s，seal3.39s，expiry2.55s，receipt2.52s，precommit2.45s，stock2.11s /3000tick。结束计时后观测3821 live envelopes、14 plans，不改变负载。
- 撤回数学helper并重新构建后最终复测127.503x /23.5366s，与此前128.454x接近；以约127–128x作为本批证据，不挑最快样本报告。
- 随后把 ConservationRow 的纯数学校验提取为 Copy 字段 helper、避开 key/String clone，真实 server127.60x，未获收益；该实验已撤回，不纳入生产diff。并行初始化尝试也撤回，因现有 continuous_shards 已按股并行。

## TDD 与边界

初始 wrapper 红测 constructor_does_not_build_the_general_multi_map_ledger：实际构造计数1、要求0；当前不构造一般三 map ledger。

新短测：已有累计 commission/transfer fee 的 release 与7种拒绝输入，对一般 ledger 的精确错误、normalized receipts、terminal/live 数与累计 audit；初始/新建 key 交错 canonical 顺序；重复批次与 terminal key 禁止复用，失败不安装 prepared 前缀；cash/share aggregate 溢出必须先于第三行守恒错误，并与一般 ledger 精确一致。

一次 Luna 边界测试初跑失败在错误描述假设不完整：MoneyError 实际携带 operand。root 修正为完整固定字符串并增加第三行错误以验证首错，不弱化断言。6项最终通过。

最终 lib binary 最新编译对应撤回后的生产实现；PrivateStockLedger6、continuous101、ledger56、envelope9、stock_stream15 的定向命令均在9s deadline/4 test threads 内执行。不同filter可能重叠，不合计作为唯一case数。

WASM、HTTP/WS、全交易日、完整workspace与前端仍未验收；save_contract既有日历错误契约失败仍未解决。当前目标1000x仍 active，下一批继续削减订单/账本全量遍历与细碎调度。独立复核记录另列，未经通过不宣称本批完成。


最新 GDB 自己启动本任务server子进程，48组864线程栈：actor多次位于ReceiptAggregation中的validate_ledger_evidence及收集完整row输入，另有DecisionResourceSnapshot::seal的BTreeMap插入与live ledger遍历。math helper无收益的产物只用于定位，后续性能验收必须重新编译当前生产文件。


全新独立 Sol reviewer 通过本批门禁，未发现需阻塞修复的问题，报告 private-stock-ledger-independent-review.md。其一次宽filter超过9s及两次零case filter不计作通过，后来正确定向套件通过；不把相关filter相加作唯一case总数。后续重新审查仍须新建agent，不能复用此次reviewer。
