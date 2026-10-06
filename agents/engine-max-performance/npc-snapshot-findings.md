# NPC decision snapshot 内部诊断（临时插桩已撤回）

固定默认 5 股 / 20,007 NPC / seed 42，16 物理核 `0-15`，Rayon16/Tokio2，3000 tick。临时 10 个 span 记录同步调用线程 wall/CPU；嵌套 span 重叠不能相加。诊断吞吐 220.467x，与无插桩候选区间接近，适合作为相对定位。

构建：诊断 release `-j16`、299s deadline，最终 Cargo exit0。期间有三次插桩脚本/源码语法错误：`DecisionSnapshot::new` scope 未闭合、重复追加 `pub mod`、错误保留分号；这些是诊断工具生成错误，不冒充 TDD 红灯。采样后源码已按 manifest 恢复，临时 module 删除，`git diff` 干净。

| span | calls | wall_s | current-thread CPU_s |
|---|---:|---:|---:|
| validate_market_view | 3002 | 0.012 | 0.007 |
| build_market_view | 3002 | 0.168 | 0.026 |
| attention_mutate | 3002 | 0.159 | 0.038 |
| working_order_capture | 3002 | 0.173 | 0.021 |
| behavior_market | 3002 | 0.119 | 0.026 |
| prepared_accounts | 3002 | 0.143 | 0.020 |
| retail_experience_install | 3002 | 0.086 | 0.024 |
| retail_analysis | 3002 | 0.865 | 0.454 |
| decision_snapshot_new | 3002 | 0.027 | 0.023 |
| experience_capture | 62,022 | 0.844 | 0.772 |

## 结论

- `capture_decision_snapshot` 的主要实际 CPU 在两处：`retail_analysis`（wall 0.865s / CPU 0.454s）和每次 due account 调用的 `CapturedExperienceObservation::capture`（62,022 次合计 0.844s / 0.772s CPU）。
- 市场/attention/working order/behavior market/snapshot 构造的单项 wall 均约 0.1–0.17s，不是主要根因。
- `experience_capture` 的 CPU 与 wall 基本相等，说明它是实际计算/clone/数据结构成本，不是 Rayon 等待；优化应针对每个账户的 positions 遍历、`RetailExperienceState` clone、BTreeMap/BTreeSet 构建、observation mutation，而不是再调调度。
- `retail_analysis` wall 明显大于当前线程 CPU（0.865 vs 0.454），部分工作在 Rayon child worker 中；它内部会重新捕获 `DecisionChainObservation::capture_for_roots`，并再次构建 `market_price_path_observations` / equal-weight market，与前面 `build_behavior_market_checked` 已构建的 price paths 存在重复只读事实。下一步最有希望的重构是同一 snapshot capture 内复用同一份 price paths / behavior market（错误顺序与 ownership 需保持），或拆分 retail analysis 的共享 observation。
- 下一实验必须先用小 fixture 写行为等价测试，再改共享路径；不能为省一次观察而改变散户 personal analysis 或行为市场首错顺序。
