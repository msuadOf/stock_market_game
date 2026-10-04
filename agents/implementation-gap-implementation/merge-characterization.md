# 合并后的受控 characterization

本记录区分实际业务变化与表示变化，不把失败断言的 `left` 当作新预期。
历史原见证保留在 `.tmp/naming-refactor-implementation/digest-evidence/current-captures/`，
无代际契约仅删除根标记的取证见 `agents/current-save-contract/implementation.md`。
此次合并没有恢复旧格式、版本字段或兼容入口。

## 独立生产者与业务证据

临时 producer 由两个测试的完整 fixture/生产调用生成，但不执行固定摘要断言，
独立写出真实 events、日终存档及逐 tick projection 原字节；真实成交、日界、
集合竞价次数、日期与股份守恒 guards 仍执行。源码、编译依赖、原字节及运行日志
留在本主题 `.tmp/merge-characterization/`，不添加产品 example。
捕获阶段的五个 fixture 进程并发，每进程外部 deadline 10000ms；
编译为独立长验证，jobs=32、外部 deadline 300000ms。

`step_skeleton` 的三个完整日共 60 个 projection，与历史原字节解析结构对照：
所有 events 与 live snapshot 均相同；SaveSlot 差异限于 `setup`、
`company_operations`、`closing_registry`、`public_library`、`ops_wiring`、`groups`。
其中包括显式公司装配/集团状态及前史公司支付、合同调度、分录、结账和报告事实。
历史付款/分录新增使数组位置和 scheduler sequence 变化，不是简单命名重排；
不得逆替换这些真实事实来宣称旧存档全等。其余根级权威字段逐结构相等。
此变化对应补缺公司经营及装配/真实报告接线，而不是市场交易规则变化。

`extraction_replay` 原 fixture 的新实测为 71 条 `OrderAccepted`、全部 Buy，
没有真实 Trade；两个 seed 都被既有 genuine-trade guard 拒绝。
增加 Player 买单或者把 float 放大不能诚实解决缺卖方，均未成为最终 fixture。
最终保持两股各 40000 股 float，使用明确开局库存再分配：按 AccountId 顺序
找到拥有至少 1000 股的 NPC，每股将 1000 股及 `initial_price.cents() * 1000`
的成本转入 Player；原持仓 `t1_locked=0`，这是开局可卖库存，不声称来自成交。
全过程不改账户现金、不增股份，使用严格当前 SaveSlot restore 校验；
不是公共日内存档、不增加保存或加载入口。day 0 Player 按昨收提交真实 Sell，
由自然 NPC 的实际 Buy 撮合。基础 seed 实际出现 3 笔 Trade，包含
`600889` 的 200 股成交（2350 分/股，Player 0 与 NPC 4/10）。
两 seed 都通过原业务 guards，事件和日终存档确实不同；没有保证自然市场成交的生产政策。

## 独立原字节摘要

FNV-1a 64 按原字节计算；step 按 tick 顺序连接 20 个完整 projection。
所有旧锚仍保留在测试历史说明中，未删除/弱化原比较与业务断言。

| 场景 | 旧当前契约锚 | 新独立 capture 锚 |
|---|---:|---:|
| step auction 0 | 16816661624066813714 | 18099143602724613260 |
| step auction 3 | 17087109400303676999 | 16192716476036528973 |
| step auction 6 | 2354296198442943349 | 2620780488067564097 |
| replay events | 5948645237561155125 | 7922886018261573110 |
| replay mid | 11144175475449247039 | 9809656468401244634 |
| replay end | 11114411633457169759 | 15011441865679768707 |

新 replay 原字节 SHA-256：
- events：`3e70a76092ccd83d0cdf245fe6bb14a122c4e43d05b8628bb18e9b2bbae05e99`
- mid：`501914c5ce3cdb8e5ecbaad7a2c4bbb653733dfd5412ac8b29b08e3c760a3ff9`
- end：`41936b82db6f252e258bf424c9c3758219fe689d87e4108ed8f70a968040bc80`

扰动 seed capture 的 events/mid/end FNV 分别为
`6600449370490963256` / `13587003593583188673` / `3236166791401903175`。
重钉后还强化真实 Player/NPC 对手方、库存转移不注资、成交费用减少总现金与
真实 NPC 买入当日 T+1 lock 的短断言。它们不改变模拟生产规则。

## 验证边界

本次是固定单 worker 的受控 characterization，不要求自由多 worker 调度逐字节全等，
也不声称旧、新经济模型全等。所有固定锚、同 seed 原字节、restore 全字段、
seed/order 扰动、日界与股份守恒断言保留。

最终 9 个短 case 全通过，每 case 独立进程、外部 10000ms deadline，
三个 replay case 并发，六个 step case 并发；最长 case 5.11s。
最终 harness 使用已构建同一 engine rlib 的 `rustc --test`，两个二进制并行编译、
`codegen-units=32`，未额外修改生产构建路径。首次增强断言误读 Player-only
live snapshot，触发真实失败；现改为逐账户 `GameSession::account()` 读取，
没有降低守恒或 T+1 要求。一次临时运行器 Node 语法错误未启动测试，
另一次 Node spawnSync 返回 EPERM，改用 shell 直接外部 deadline 运行；
均未计为通过。rustfmt 与限定 diff whitespace 检查通过。
非作者 `review_merge_runtime` 已完整复审两测试与此记录，并独立读取 producer/capture，
复算全部新 FNV/SHA 与60个历史 events/live snapshot 对照，三项独立门禁通过。
root 随后实际 Cargo workspace all-feature 编译通过，并从 Cargo 产物运行三个 replay
和六个 step 短 case 全绿，suite 分别5.18秒和3.30秒，均有10000ms外部deadline。
此记录不冒称完整回归通过。
