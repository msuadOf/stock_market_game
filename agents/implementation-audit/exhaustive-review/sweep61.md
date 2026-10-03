# 历史实现穷尽复核 sweep61：Mobile、Remote 与请求 owner

## 全文范围与基线

- 产品基线 `b76ece3`，merge产品相同；承接根AGENTS/principles。本批只写本文件，未改产品、Git或跑测试。
- 连续全文读取 `agents/oop-refactor-implementation/frontend/mobile.md` **42行**、`remote.md` **73行**、`request-owners.md` **25行**，共 **140行**，三篇均无跳节。
- 三篇是行为保持OOP记录，不是批准保留缺陷的产品决策。当前正式UX/ADR优先。既有G13/G14/G03–G05等仍存；另发现Remote lifecycle的两个候选（C61-1/C61-2），尚未自行分配总账G编号，交父任务独立核实与去重。

## Mobile 逐章

| 原文章节／行号 | 当前caller与事实 | 判定 |
| --- | --- | --- |
| 范围3–8 | 行情呈现、算术均价、5/20交易日聚合按旧行为搬迁，prototype不是权威交易 | 不因class提取宣称修复图表口径；Q07/Q08仍待明确，prototype硬编码不当真实A股业务 |
| N08 10–16 | `apps/web/src/mobile/MobileStockDetail.tsx:102` 创建MobileIntradayProjection，103行消费visible/scale/平均/进度/trades，105行volumeMarks，133行auctionLine；`market-model.ts:625` class | class真实生产接线已有；owner提取无漏caller |
| N08最近七笔／时刻，13/15 | `market-model.ts:655`仍 `inputs.trades.slice(-7).reverse()`，663行全体使用elapsedMinutes-1的tradeTime | 原文诚实保留旧行为，正式最近成交/真实成交时间要求仍 **G13/G14**；不能用“新projection测试通过”核销方向与时间错误 |
| N08 phase volume／均价，15 | phase分开缩放、null竞价价格仍保量为现行projection能力；输入连续volume依赖上游 | 分钟量上游聚合错仍 **G10**、日界采样残留仍 **G11**、volume方向/样式仍 **G12**；并非移class即修，算术平均不是VWAP **Q08** |
| N09 18–24 | `MobileStockDetail.tsx:78`真实useIndicatorResults，81行MobileKlineProjection.fromInputs，83行MA、84行KDJ、94行candle/wick、96行volume、97行error/pending | Rust KDJ→hook→projection→SVG已接；`market-model.ts:726`全历史MA后切window；5/20游戏日聚合 **Q07**仍不等于公历周/月，MA/均价口径 **Q08** |
| prototype 26–31 | 文档记录controller在HTML交互script直接构造，DOM样例/动画属于设计原型 | 没有正式产品承诺要求prototype进engine；未知panel保现状不是交易错误；DOMfixture不等同真浏览器验收 |
| 红绿/限制33–42 | 当时短测试确记录owner缺失、迁移结果；41–42行明确没跑完整/浏览器/tsc及独立review待总协调 | 本轮未重跑，不据历史Node shard计数宣称当期构建/视觉验收。后续总协调复核记录优先，不把这个当永久独立门禁缺失 |

## Remote 逐章

| 原文章节／行号 | 当前caller与事实 | 判定 |
| --- | --- | --- |
| 范围3–10 | 只迁已有owner，CommandQueued仅入队、未改engine/Redux/schema | 不能用行为保持记录批准旧故障；当前ADR0010及错误展示仍约束adapter |
| N01 12–19 | `remote-host.ts:47`唯一ReportQueryContext；285行companyFor、288行validateReport，baseline80行及load成功路径invalidate | 页/ID登记和跨generation失效生产接线存在，非只有单元helper。信息查询与本人NPC获知分开，不新列G |
| N07 21–26 | `remote-host.ts:48`唯一RemoteCommandRegistry；209行submitIntent检查socket，212行取id，214行register，215行send；100–109行queued/gateway resolution；55–62行fail rejectAll | CommandQueued关联已接，不能称成交确认；class自身resolve/reject前delete由 `remote-command-registry.ts:17/24`实现 |
| N07残余27 | synchronous send throw会使Promise executor reject但map条目仍在；dispose无rejectAll | **C61-1** lifecycle请求未完结，详下。同步send与dispose须区分：前者调用promise已reject，不能错误称它也永远pending |
| R2N02 29–36 | `remote-host.ts:46`唯一RemotePublisherState；ownerinstallBaseline52–57更新epoch/cache/awaiting；start154、connect117、message75、fail55、queries233以后均使用owner | 所有权迁移已接。cachedBaseline只安装baseline，protocol90行不推进cache；start161行重送旧baseline仍 **G04** |
| R2N02残余37 | message/close有identity guard，129行onerror直接fail没有identity guard | 旧socket error可使新连接fail；这是连接生命周期/恢复 **G05**的追加边界，不另计独立后端功能 |
| R2N02残余38 | requestResync65–72先beginResync/send，再存单槽waiter；没有timeout或合并/排队；owner34行直接覆盖 | **C61-2** 重同步旧Promise丢失，详下；load单独138–150行5000ms timer不修复一般refreshBaseline并发 |
| R2N02 queries39 | orders/diagnostics用generation+baseline epoch；save233–241只generation/awaiting/disposed | 公共save是否需同generation内epoch pin取决于ADR0025保存候选语义，不能未经证据一律增强条件；作为范围边界，不新列G |
| R2N02 tests40 | 实际adapter特征测试明确锁旧error、cache、单槽覆盖、send throw及query guard | 此类characterization可保重构行为但不证明缺陷符合正式契约；需区分“提取未变”与“现行产品正确” |
| 验证42–58 | 59case、oxlint历史范围；58行未全量/build/tsc/E2E | 本轮不重跑，无法从owner测试推出浏览器WS可鉴权；`remote-host.ts:120` remoteWsUrl仍归 **G01**、readSpeedMetrics198行不附token仍 **G02** |
| 未完成60–64 | 迁caller完成而fullreview待总协调，64行明确旧残余未修 | 不把“无待实施owner动作”当旧残余核销；当前request lifecycle候选可独立从源码判断 |
| fixture修66–73 | fake HostFailure.where和可选capability narrowing修的是测试类型 | 没改变生产失效路径；不据“tsc交总协调”声称永久typecheck红 |

## 请求 owner 逐章

| 原文章节／行号 | 当前caller与代码 | 判定 |
| --- | --- | --- |
| R2N01 3–9 | `components/useIndicatorResults.ts:27`capture、28行pending、29行microtask calculator，31/35行resolve/reject；39行cleanup invalidate；`indicator-results.ts:44`request，69/79行旧ticket返回null、72行严格series parser | 真hook→owner→parser→React state已接；disabled42/43行idle/unavailable，旧响应不会覆盖新输入。catch75行生成显式error record，非吞错fallback。Rust MACD/KDJ真源不变，batch生产接线仍G17 |
| R2N04 11–16 | `host/company-query-coordinator.ts`的query/queryReportById/installBaseline/dispose使用唯一CompanyRequestRegistry；209行generation/disposed+ticket匹配，111/140行finally仅清当前ticket | 同keyforce旧finally不删除新请求、缺capability明确unavailable已有；没有发现该owner漏production caller。Redux/queryDTO仍由coordinator和共享normalizer约束 |
| 验证18–25 | runtime版本/10s两进程/fixture lint修及总协调typecheck是历史记录 | 本轮不运行DOM/tsc；hook隔离dispatcher不冒充真实浏览器挂载；无新增产品范围 |

## 新候选详细证据与反证

### C61-1：Remote dispose 丢失未确认写命令的完成路径

- 原文：remote.md:27、64明确“dispose不主动reject pending command”，是OOP保持的残余。
- 当前生产链：UI `apps/web/src/app/useTradingCommands.ts:80`等待submitIntent、94行等待撤单；`remote-host.ts:214`register waiter，215行发送。若已发送后尚无CommandQueued而host被dispose，168–187行仅关socket/清callback/拒baseline waiter/DELETE session，**没有commands.rejectAll**。
- socket close的132行要求 `!disposed` 才fail；message的76行对disposed直接忽略。故dispose之后queued/error既不能完成命令，普通close也不触发55–62行已有rejectAll，原命令Promise永久悬挂且registry保留条目。
- 现有characterization `remote-state-contract.test.ts:145`在154–157行显式assert dispose后settled=false，提供静态行为证据；本轮没有执行该test。
- 依据：ADR0010:60规定有副作用命令等待宿主确认；`docs/error-handling.md:3/68`要求错误显式可见。dispose并不保证命令未执行，修复不能伪装成业务“拒单”；应明确“会话已销毁，无法再取得确认/结果未知”，不要虚构CommandQueued/成交。
- 反证区分：同步send throw151行对应已登记失败条目，外层Promise本身被executor reject，不属于同一个“无终止响应”触发；fail已经能rejectAll但dispose没有caller；G05连接恢复未完整实现不能自动覆盖主动dispose清理，两者触发不同。本候选交父任务确认是否独立G或并入更完整的宿主lifecycle条目。

### C61-2：Remote重同步单槽覆盖令先前调用永远pending

- 原文：remote.md:38明确单槽覆盖不settle旧waiter，64行保留该残余。
- `remote-host.ts:243`公共refreshBaseline await requestResync；65–72行每次send Resync并beginBaselineWait。`remote-publisher-state.ts:34`直接赋新waiter，旧resolve/reject丢失。一次baseline在message82行仅resolve最新waiter；dispose177行也只能reject最新槽，旧调用以后没有任何完成路径。
- 现有 `remote-state-contract.test.ts:119`真实adapter fixture两次refreshBaseline，125行收到合法baseline、126行第二成功、127行第一仍未完成；静态证据，不宣称本轮复现运行。
- 反证：actor已经返回完整baseline不救已丢失Promise；load5000ms timer只适用于load恢复，不能覆盖一般refresh。requestResync internal generation/resync分支也参与同一个槽，可与public刷新相互覆盖。无需改变交易优先级/WAL，需明确合并waiters、拒绝重复调用或串行完成之一。
- 与G04不同：G04是暂停继续缓存重送/回退；这里即使新baseline内容完全正确，旧await也永远不结束。正式ADR没有授权单槽覆盖后静默遗失调用结果；但如何定义并发refresh语义仍应由父任务独立核实，暂不自行编号。

### 已排除及并入已有项

- 旧socket.onerror缺identity guard并入G05边界；同步send留下死登记保留为C61-1邻接清理边界，不夸大为调用promise未reject。
- push/pull列capability但没有GetFrame循环仍G03；pause/start重送缓存仍G04；token/speed仍G01/G02。
- Mobile recently-trades反向和共享tradeTime仍G13/G14；算术均价与5/20交易日为Q08/Q07；class不改变上游G10/G11/G12。
- company/indicator request ownership已真实接，未发现新增漏caller；prototype/SSR隔离fixture不是跨平台视觉验收。

本批不改交易、价格/股/手单位、存档或权限；候选涉及异步请求lifecycle，不要求可靠投递/WAL或保证dispose前命令没有执行。父任务统一独立diff复核、去重与总账分类。
