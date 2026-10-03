# Sweep60：frontend App、chart、host 后续工作记录全文核对

## 范围与阅读

- 已连续全文读完 `agents/oop-refactor-implementation/frontend/app.md` 82 行、`chart.md` 109 行、`host.md` 41 行，共232行，包括文件清单、测试限定、未完成复核与明确另批行为修复。
- 基线 `4ad5a2e`，产品树按父任务说明等同b76ece3；只新增工作记录，没有产品修改、Git写或测试/长验收。
- 依正式 `UX-CONTRACT.md`、`DESIGN.md` 与 ADR-0010/0025 判产品行为；owner提取工作记录是迁移证据，不自动覆盖正式UI/宿主要求。补读 frontend/review 的相关核销段，确认 FR01 已修以及dispose/fatal与半初始化rollback明确在scope exclusions。

## 条款矩阵

| 原文条款族 | 当前状态与代码/caller |
|---|---|
| app.md:3–7 范围、A股金额/数量/T+1、未报告全批次review完成 | 迁移范围落盘；历史测试/环境结果不冒充本轮验证。数量预检在 `apps/web/src/app/useTradingCommands.ts:63`/`:66` 继续用股及可卖量，符号价格经原intent建造。 |
| app.md:11–23 SessionHostLifecycle唯一effect owner、取消/清理顺序、Shell refs/协议authority、host创建原Promise | 已接线。`apps/web/src/App.tsx:287` 使用hook；`useSessionHostLifecycle.ts:79` stopCurrentSession，`:97` 取source seed，`:98` 创建，`:102` 对已取消异步host dispose；`:139` 连接真实start，`:202` 返回同一对象，`:226` 装stopStartupRef。不要求把coordinator权威状态搬进class。普通新局仍用DEFAULT_SEED，归G20。 |
| app.md:25–35 SaveCommands六caller、共用gate、等日终idle、授权不日内写档 | 已接线。`App.tsx:352` 构造统一save facade；`useSaveCommands.ts:63` recover、`:95` load、`:146`授权文件、`:161`文件load、`:211` newGame；`:93`/`:155` 明示日终保存。恢复/加载门禁沿用共享refs，不是第二个存档authority；失败在`:139`/`:205`显式notice。正式日终语义依ADR0025，不因方法handleSave名字恢复日内保存。 |
| app.md:37–46 TradingCommands单表单、玩家活动委托列表、稳定clear、确认仅提交 | 已接线。`App.tsx:212`唯一hook；`useTradingCommands.ts:80` await宿主submitIntent，`:82`仅说已提交；`:94`真实撤单，`:102`入队失败notice。预检扣T+1和挂卖占用。当前仍全局notice、非字段级关联，归G22；owner迁移不核销该UX缺口。 |
| app.md:48–54 SpeedMetricsPolling timer/cancel、立即读取、1秒重试与load gate | 已接线。`App.tsx:324`调用；`useSpeedMetricsPolling.ts:20` capture共享gate，`:25`检查current后写结果，`:27`显错，`:29`1秒重试，`:34`cleanup取消timer。Remote测速本身凭据缺失归G02；计时owner存在不证明远端HTTP已经鉴权。 |
| app.md:56–63 PausePreferences loaded一次、host-first、storage getter异常边界 | 已接线。`App.tsx:310`调用；`usePausePreferences.ts:21`唯一loaded，`:25`加载一次，`:37`先向host发setPausePreferences并显式catch report；`:59`/`:64`惰性sessionStorage getter留在lifecycle异常边界。没有重复设置authority。 |
| app.md:65–82 测试helper、统一tsc返修、未运行浏览器/全量与review | 历史证据限定保留。完整EngineHost fixture与hook-runtime不构成真实浏览器；frontend/review后续核销拥有独立复核记录。不能仅此历史文件“等待父任务”宣称当下无实现；本轮没有重新执行它们。 |
| chart.md:3–11 保留原坐标/量/颜色/权威日K | owner迁移承诺已满足；原行为保留不等于原算法符合全部正式UX。当前 `market-chart-projection.ts:14`帧累计量差、`:15`buy恒true仍归G10/G12；按日内槽位合并无正常日界隔离归G11。 |
| chart.md:13–33 MarketChartProjection五字段、freeze、无额外authority、hook/Provider/callers真实迁移 | 已接线。`market-chart-projection.ts:56`统一owner，`:63`upsert、`:67`rebuild、`:74`snapshot、`:89`reset、`:103`只读索引；`useMarketChartRuntime.ts:17`唯一惰性实例，`:22`/`:23`getter；`MarketRuntimeProvider.tsx:61`actions接线，`LocalRefreshViews.tsx:87`/`:173`真实消费。冻结新投影值，不冻结借用snapshot；`:94`/`:99`每次selected返回新数组的刷新边界仍归G31。 |
| chart.md:35–49 PriceChartRuntime拥有第三方chart/series/observer/listener、effect生命周期与指标状态 | 已接线。`components/PriceChart.tsx:61`每挂载独立create，`:64`cleanup dispose同一对象，`:70`/`:74`真实update；`price-chart-runtime.ts:52`createChart，`:89`副图，`:112`observer，`:116`listener。权威日K只窗口裁剪（`:131`），不把展示数据伪装为引擎K。 |
| chart.md:45、50–53 半初始化rollback另批；FR01逐项remove成功清字段与失败重试 | FR01已核销；半初始化资源边界仍明确延期。当前构造在`:52`创建主图、`:67`addSeries，然后`:90`副图创建等都可能抛错，`PriceChart.tsx:61`返回前尚无effect cleanup，因此没有该独立rollback。原文及frontend/review:99/:104明确没有授权它作为本次等价提取目标；作为后续资源行为候选保留，不能新增“提取漏接”G。 |
| chart.md:55–69 MarketGridRowSynchronizer latest/submitted、ready/effect、destroy借用解绑、throw不前移 | 已接线。`MarketGrid.tsx:47`唯一实例，`:58`onGridPreDestroyed，`:162`传事件；`market-grid-row-synchronizer.ts:23`render登记，`:27`attach从实际initial基线，`:38`diff、`:42`提交，成功返回后`:45`才更新submitted。名称submitted明确不是AGGrid异步完成；dispose仅`:35`解除api不假称取消队列。 |
| chart.md:71–109 suite/red-green/FR01返修/类型mock与完整文件清单 | 测试与工作记录无新的产品承诺。按mock/SSR覆盖限定处理，原文不声称浏览器、E2E、全回归通过。当前所列owner/真实组件都存在，不依据旧源码text守卫位置判未实现。 |
| host.md:5–14 WorkerRequestScope唯一pending/sequence、所有command共用、generation关联、once cleanup | 已接线。`host/worker-host.ts:133`唯一scope；各生产helper接收该scope，`:55`/:60/:65/:71等用真实command；`worker-request.ts:26`统一owner，`:54`/`:55`双身份过滤，`:45`幂等cleanup，`:64`原10秒timeout。nextRequestId是生产请求caller使用的方法，pendingCount用于测试观察符合范围。 |
| host.md:12–14 dispose/fatal不立即reject、同步postMessage异常不立即cleanup、restore不主动cancel | 当前仍为明确延期行为。`worker-request.ts:71`/`:72`保留同步throw后timeout清理；`worker-host.ts:269` dispose只转lifecycle。frontend/review:182/:224复核明示这些是原动作排除的另批行为修复，不把它们描述为本批“已修复”，也不因新scope有pending Map就倒推必须新增dispose契约。 |
| host.md:16–25 TauriTimelineState四字段、string/BigInt、listener过滤、baseline/refresh/load/query guards | 已接线。`tauri-timeline-state.ts:19`唯一owner，`:14`精确BigInt递增，`:58`epoch/generation查询guard，`:64`初始安装，`:70`refresh，`:78`restore，`:86`dispose；`tauri-host.ts:116`/`:125` listener滤timeline，`:223`/`:233`真实替换。不同query guard及parse前timeline/generation部分写入保留原边界，不声称完成统一晚到响应取消。 |
| host.md:20–23 start缓存交付、dispose与晚到load/refresh | 迁移接线已有；正式协议问题仍归G04。`tauri-host.ts:155`每start读取baselineForDelivery，`tauri-timeline-state.ts:37`从缓存交付，cache未随delta推进；App暂停后继续仍调用start，因此旧baseline可再送。ADR0010:56仅允许初始化/读档/显式重同步；不能以此工作记录“等价”核销正式契约问题。 |
| host.md:27–41 Money/shares/T+1/日终契约、精确短测/lint、等待独立review | 无新领域规则或存档字段；短测试验不替代三宿主矩阵。Worker uiFrame背压G18、Tauri固定高倍率16ms聚合G19在正式ADR0010:66/:68仍未被frontend owner提取实现。 |

## 新候选反证与后续边界

1. **“新类存在但App/Grid/Chart/Tauri无caller”未成立。** 本轮从App.tsx、PriceChart/MarketGrid到host facade逐个追到真实构造、方法和cleanup；它们不是只出现在mock中。
2. **“Grid submittedRows等于错误宣称应用完成”未成立。** owner明确只记录submit成功，throw不前移，队列归AGGrid。除非另有正式要求承诺事务完成后的回执，不新增异步完成authority。
3. **“FR01仍在”未成立。** 后续review明确修复逐项remove部分失败的记账，当前runtime按成功删除同步更新句柄；不借FR01扩大成创建全过程rollback。
4. **创建失败资源清理、Worker立即失效pending确实尚未实现，但属于明确单独延期项。** 两篇原文直接排除等价提取范围，后续独立review也接受scope exclusions；在总账“独立行为修复/待定”边界留证，而不以它们制造新的已确认产品G。即时错误仍可见，Worker同步throw会reject原Promise，资源在原timeout回收，不能写成静默吞错或永久无响应。
5. **Tauri部分失败与晚到响应的guard不统一仍是边界。** 恢复parse失败先换generation/timeline与dispose后晚到refresh/load按旧模型保留；文档没有宣称已修正其行为。正式已确认stale baseline问题继续归G04；更广泛原子安装/取消需要具体失败契约与测试，不能凭class拆分自行改变。
6. **“工作记录测试证明全部UX符合”不成立。** 本轮保留G02/G04/G10/G11/G12/G18/G19/G20/G22/G31；getter冻结与owner完整并不取消分时量、日界、红绿、刷新隔离及宿主协议缺口。未重复新增这些编号。

结论：232行全部条款已核对。没有确认总账之外的新产品遗漏；主owner接线与FR01返修已有代码，三类明确后续资源/失败边界据实保留。SSR、fake chart/Worker与历史短测不能冒充真实浏览器、统一类型检查或本轮测试通过。
