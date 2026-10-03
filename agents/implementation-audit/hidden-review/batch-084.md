# 批次 084 独立复核

## 阅读完整性

| 文件 | 行数 | SHA-256 | EOF |
|---|---:|---|---|
| `agents/oop-refactor-audit/exhaustive/reviews/web-05-evidence-final.md` | 27 | `23e5d77725f380be7f2d06166193fe956529d13344ffaebedab0ace7e21f6004` | 是，连续全文至末尾 |
| `agents/oop-refactor-audit/exhaustive/reviews/web-05-model-final.md` | 20 | `a8827e9d4243655970530755d9de7c103d9b1ee30b9b31cd6968feb577a64857` | 是，连续全文至末尾 |
| `agents/oop-refactor-audit/exhaustive/reviews/web-06-final.md` | 28 | `dcc6b7023f44a7c8c44f3fbd3b30c60111726f513b256bc2029261fa8c0aa63e` | 是，连续全文至末尾 |

已阅读根 `AGENTS.md`、`docs/principles.md`、`docs/open-questions.md`，以及当前相关 ADR-0004、ADR-0007、ADR-0010、ADR-0025。基线 HEAD 为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。静态核对当前源码 owner、caller、consumer 和主账 G01–G68/Q 对照；未运行测试、构建或 Git 写操作，未查询官方规则。

## 结论

三篇旧复核记录作为各自限定范围内的审查记录仍可采信：web-05 evidence delta 核对了六个具名条目；market-model delta 核对清单/模块增补及 collector 参数归属；web-06 复核了 `loadViaUpload` 的 Promise 完成语义并保留 focus listener 清理风险。它们均不是当前产品完整验收，也没有理由核销实现主账缺口。

### Owner、调用链与边界

| 范围 | 当前核对 | 判定 |
|---|---|---|
| `MobileRunToggle` / `MobileSpeedSelect` | `App.tsx` 与 `MobileStockDetail.tsx` 传入 `running`、`speed` 与回调；组件本身是受控视图，按钮点击委托给调用方，速度字符串在 callback 边界转成 number。 | 旧 evidence 复核所述 props 与 UI 行为吻合；无额外状态 owner。 |
| `writeDayEndTargets` | 日终协调器调用 `Promise.allSettled` 扇出各 `DayEndTarget.write`，汇总 label 状态；无目标显式抛错，任一写失败时抛带首错 cause 的 `AggregateError`。专项目标测试存在。 | 旧复核对一次性 I/O 编排、错误汇报边界的描述与当前实现吻合；ADR-0025 的仅日终持久化边界仍是决定依据。 |
| `MinutePointCollector` / `AuctionPointCollector` | 当前定义在 `mobile/market-model.ts`；全仓生产 hook 没有实例化它们，测试调用集中在 `market-model.test.ts`。`useMarketChartRuntime.ts` 实际持有 `MarketChartProjection`，输入经 `market-chart-projection.ts` 的 `upsertFrames` 投影。 | 默认值与两类量口径可作为 collector API/测试事实；不得将其描述为生产行情接线、完整成交量或方向实现。 |
| 生产分时投影 | `MarketChartProjection` 的生产实现按帧计算累计量差并通过 `mergeMinutePoints` 按分钟更新；连续点 `buy` 固定为 `true`，集合竞价方向仅由指示价存在性决定。 | 与主账 G10–G14 仍缺的事实相符；旧 market-model review 的“保留两个 collector”结论不覆盖本生产 consumer，也不能核销这些 G 项。主账关于图表量、日界、方向及逐笔字段的详细现状继续适用。 |
| `loadViaUpload` | 文件 `change` 无文件时和 focus 延迟兜底时写 `settled`；成功解析 `resolve`、读/解析失败 `reject` 均未更新该标志，但 Promise 结果本身只完成一次。focus 监听器只有触发后因 `{ once: true }` 移除，成功或失败后未主动清理未触发 listener。 | 旧 review 对“不存在已完成 Promise 结果被覆盖”及 focus 只是近似取消信号的结论正确；独立 listener 生命周期风险仍存在，旧 review 已明确登记，没有被错写成已修复。 |

## 总账、ADR 与语义边界

- G01–G68 主账各章仍按当前状态有效；本批没有实现改动，不核销任何 G 项。尤其 G10–G14 的生产分时投影缺口不能由 web-05 的旧 evidence-only 或 collector 清单复核替代。其他 G 章节与本批三份历史文档没有可建立的直接 caller/consumer 对应，不能因都属于 Web/OOP 而强行映射。
- `docs/open-questions.md` 的 Q7（持久化）与 Q8（模拟确定性）已有决定和现行边界，不因这些历史审查改变。实现审计主账里的内部 Q07/Q08 是另一套编号，分别记录周/月 K 线聚合与移动详情均价口径；它们仍按主账状态处理。本批没有足够证据改写任何一套问题的结论，也未发现三篇来源能裁决它们。
- ADR-0004/0007 的 Redux 与前端组件架构、ADR-0010 的宿主/更新协议边界、ADR-0025 的日终存档规则与审查范围相符；没有把组件、浏览器上传分支或日终写入协调器提升成新的 engine authority。
- 本批是静态审查记录，不改变交易制度、金额/股数单位、撮合或 A 股市场阶段语义；无需新增官方规则主张。速度控件的展示输入不是交易规则或倍率执行验收。

## 发现与限制

- 未发现三份旧 review 在其明确的 delta 范围内有事实性反证；market-model review 对测试 collector 的 API 归属与生产 hook 未使用 collector 的说法一致。
- 需要防止复用旧 review 的“通过”措辞时扩大范围：web-05 evidence 记录明确排除完整 batch，web-05 model 只重审单条 delta，web-06 继承旧批次结论且只针对 unit-056。本次 caller/consumer 对照也没有重新验收全部 UX 或交易显示要求。
- listener 未清理是来源已披露的独立边界风险；分时生产路径的 G10–G14 缺口则已由现行主账登记。本批不新增重复缺口或候选。
- 未运行测试、构建、浏览器验证或官方规则查询；不宣称行为验证或完整回归通过。
