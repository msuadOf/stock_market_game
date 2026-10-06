# 股票身份数值化（StockId）落地与基准（E8 阶段一）

## 改动

- `refactor(engine)` c79f051c：StockId(u32) + StockCatalog（id 按代码升序，id 序即代码序）；
  引擎核心/存档/事件/证据投影全量数值化；公司域范围切割（翻译缝 issuer_of_stock）；
  RNG 派生保持代码字符串（价格路径不变）。230 文件 +2957/-1996。
- `refactor(engine)` c6cd8761：escrow 验证 harness 对齐。
- `refactor(server)` 7720a62d：三宿主 wire 数值 id + ts_rs 再生成。
- ADR 0041、string-free-inventory.md、e8-string-sweep-progress.md 随批入库。
- 测试：engine lib 1311 过/8 败（基线 dde740c5 经临时 worktree 对照**逐一相同**）；
  集成/公司系失败同样全部基线固有（company_* 两例为基线编译破损掩盖的既有失败）；
  server/web-wasm 全绿（仅基线固有的 actor_market 一例）；workspace 编译 0 错误
  （desktop generate_context! proc-macro 崩溃为 HEAD 既有环境问题）。

## 基准（锚定交错配对，A_start,(H,C)×6,A_end；0-15 核占用 <16%；worker=4 生产默认）

- H = dde740c5（sweep 前）冻结 binary；C = 7720a62d+c6cd8761（probe SHA 77e42042…）。
- 锚漂 1.87%（≤5% 门槛过）；6/6 配对比值 >1；mean(d)=**1.2273（+22.73%）**；
  H 区间 [232.4,242.0] 与 C 区间 [285.9,300.3] **完全分离**；五重判定全过。
- 逐对：1.2246/1.2298/1.2074/1.2604/1.2113/1.2301。
- 基线归档 `.tmp/perf-anchors/dde740c5/` 与 `.tmp/perf-anchors/stockid-candidate/`；
  逐跑样本 agents/engine-max-performance/e8-*.json/.err。

## 结论

- 数值身份收益远超 callgrind 保守外推（预估函数级上限 1.088x、类目级 1.266x——实际落在这
  两档之间偏上，验证"字符串机械成本碎片化"画像）。
- 与既有优化正交：这是主线迄今最大单笔收益（+22.7%），叠加后正式口径 ~236x→~290x。
- 后续（清单未竟项）：B 结构化错误（数字错误码 + ts_rs 数字 enum + 前端文本表）、
  C CompanyId/venue/frozen/光标数值化、A 表复盘剩余行。
