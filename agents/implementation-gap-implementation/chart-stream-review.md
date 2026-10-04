# 分时与逐笔独立复核

## 身份、范围与依据

- 复核者：`review_chart_stream`，未参与产品实现；不修改产品，不运行完整回归。
- 范围：G10、G11、G12、G14、G31；按总账 `implementation-audit-2026-10-02.md` 与 `reaudit-ui.md` 的完整原文判断，不以 owner 存在或旧 collector 单测核销。
- 已读根 `AGENTS.md`、`docs/principles.md`、`docs/architecture.md`、`docs/open-questions.md`、ADR-0009/0010 与 `UX-CONTRACT.md` 的分时、量柱、日界及局部刷新要求。
- 完整审查分时 owner、hook、Provider、PriceChart readonly 接口、protocol effects/reducer/types、TradeEvent、market-model、MobileStockDetail TSX/CSS 与对应测试差异；App/LocalRefreshViews 仅检查真实消费者接线，其余并行改动不在本批认证范围。

## 初审有效发现

1. **G12/G31：竞价同槽不变采样翻色并换引用。** `apps/web/src/app/market-chart-projection.ts:35` 的 `history.findLast` 包含待覆盖槽自身。实际短命令复现：tick 1 指示价 1300、tick 7 指示价 1200 得槽 1 `buy=false`；tick 8 仍为 1200、量仍为 10 后改成 `buy=true`。同六秒槽价格与量都未变化，不应改变方向和引用；tick 8 回升至 1250 仍低于前槽 1300，却被误标上涨。已通知作者补失败测试并修复。
2. **G11：BeforeOpen 屏障仍保留旧日分时。** `apps/web/src/app/useMarketChartRuntime.ts:54` 对 CivilUpdate 无条件 `rebuildHistory(update.intraday)`。`packages/engine/src/session/protocol/civil/mod.rs:200` 在下一日 Trading 时加入 BeforeOpen，但 `:224` 仍携带刚收盘日的完整帧，tick 不前进。仅按 frame.tick 判断日界不能清这条生产路径；直至次日首 tick 仍展示旧日点。既有 `market-chart-runtime.test.ts:102` 还断言这一旧点保留。总账 G11 已明确列出相邻 AfterClose＋BeforeOpen 反例，不能用普通跨 tick 批次测试替代。已通知作者修生产 hook 和真实 CivilUpdate 测试。

## 初审核销判断

| G | 当前判断 | 依据 |
|---|---|---|
| G10 | 可核销 | 连续量按逐帧累计量差加到同分钟旧量；Completion.matched_volume 初始化连续基线，量单位仍为股，在 renderer 换算手，不混入竞价量。 |
| G11 | 未闭环 | 普通同批次跨日已清缓存并保留新日帧，但上述 BeforeOpen production 反例仍需修复复核。 |
| G12 | 未闭环 | 连续分钟方向及 SVG 红空心/绿实心已真实消费；竞价同槽方向反例仍需修复复核。 |
| G14 | 可核销 | Trade effect 从所属 frame.tick/CivilUpdate.tick 获取时间；Redux 原样保留，详情按每行 formatTradeTime(trade.tick) 显示，不再借当前时间。缺字段明确显示“成交时间缺失”，非法 tick 显式抛错。 |
| G31 | 未闭环 | 无关股票、同分钟无变化分时与同内容日 K 已复用冻结数组，hook setState 与 Provider memo 可沿引用隔离；竞价同槽不变翻色导致引用变化仍需修复复核。 |

## 大 A 语义、必要性与边界

- 本批是权威行情的展示投影，不修改交易受理、撮合、T+1、费用、证券类别或委托单位，不新增交易制度，无需把视觉修复冒称交易所制度认证。
- 沿用 ADR-0009 默认 900 tick 开盘窗口、240 交易分钟及压缩午休的游戏映射；时间 helper 明确覆盖 09:25、09:30、11:30、13:00、15:00 与次日，未自行裁决 Q04/Q07/Q08 或扩展配置模型。
- `buy` 在此仅作为价格方向展示字段，不能被解释为真实主动买卖；保留 null 竞价指示价、真实零量和未到达槽位，不伪造价格/成交。
- 稳定冻结 readonly 历史是 G31 必需的 owner 边界，没有把 Redux/protocol authority 搬入展示层；TradeEvent 可选 tick 仅兼容已有展示输入，不修改 Rust/wire Trade。
- exact-retry 不追加 trade effects；baseline/cursor/generation 校验仍在 coordinator/reducer，错误经 App onFailure 可见出口，不以空数组静默吞错。正常空图共享冻结空数组，与协议失败状态不同。
- 两项有效发现必须修复后再次审查才能宣布本批整体完成。

## 已执行短验证

- 5 套定向命令：`timeout 10s node --experimental-strip-types --test --test-isolation=none --test-timeout=10000 --test-concurrency=2` 加 market-chart-projection、protocol-effects、trade-time、protocol-reducer、market-model；52/52 通过，进程约 0.41 秒。并发参数显式为 2；无隔离模式不冒称多进程利用率。
- 两独立 SSR 进程：在 `apps/web` 执行 `timeout 10s node --experimental-strip-types --test --test-timeout=10000 --test-concurrency=2 src/mobile/mobile-component-render.test.ts src/app/market-chart-runtime.test.ts`；两文件级测试通过，进程约 0.84 秒。Node 输出只汇总 2 个文件级结果，不冒称其全部 case 数；其通过不能否定上面未覆盖反例。
- 未运行完整回归、浏览器像素/辅助技术矩阵或全项目编译，不宣称这些验证通过。

## 修复后再次复核与最终结论

- 作者确认两项发现均先增加失败断言再实现修复；失败过程依据其 `chart-stream.md` 记录，不冒称复核者亲自执行了作者的红灯阶段。
- 第一项已修：`market-chart-projection.ts:36` 仅选 `sample.time < time` 的前有效竞价槽；新测试覆盖同槽平价的数组 identity、方向不翻转，以及槽内回升但仍低于前槽时保持下跌。原实际反例已纳入通过的 53 项定向短测。
- 第二项已修：`useMarketChartRuntime.ts:54` 对含 BeforeOpen 的 CivilUpdate 显式重建空展示历史，不修改 protocol authority；安装 snapshot 的权威日 K 继续执行。真实 production SSR 测试改为断言 prices、auctions、history 清空且日 K 保留，并新增 AfterClose-only 进入非交易日仍保留真实收盘图的反向边界。普通同批次跨交易日后继续保留新日采样的测试仍通过。
- 再次执行同一五套定向命令：**53/53 通过**，进程约 0.35 秒；再次执行双独立进程 SSR 命令：**两个文件级结果通过**，进程约 0.84 秒，并发仍为 2，case timeout 与命令 deadline 均为 10000ms。
- **最终 G10、G11、G12、G14、G31 均可按本次总账具体缺口核销**；初审未闭环判断仅记录发现时状态，不是最终状态。两项有效发现已修复并经过非作者再次复核，没有遗留本批阻断发现。
- 图形颜色/形状经真实 SSR 和 SVG 属性检查，但未执行浏览器像素测量；G31 核销仅指未变当前股票历史数组不换引用的约定，不声称 measured FPS/全部父组件零重绘。未认证正在并行施工的全 App async lifecycle 或其他 G 项。
- 审查完整范围补记：此次修正还包含 `apps/web/src/app/market-chart-runtime.test.ts`，逐笔新增未跟踪文件 `apps/web/src/mobile/trade-time.test.ts` 亦已完整阅读与执行。indicator calculator 注册 token 清理、指标请求 generation gate 与 baseline cursor 校验未被本批更改，既有隔离机制保留，不将其当作新实现功绩。
