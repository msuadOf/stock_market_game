# Sweep 65：hosts 性能、Account caller 与 actors 后续工作记录

日期2026-10-03；产品基线b76ece3，工作树4ad5a2e为审计merge。连续全文读取三篇，共169行：`agents/oop-refactor-implementation/hosts/performance/status.md`59行、`hosts/review-account.md`39行、`hosts/review-actors.md`71行。已遵循根AGENTS/principles，只增加本记录；另按父任务授权修正sweep14 C14-2证据限定。未修改产品、Git写、编译或运行长测。

## 全文章节与条款矩阵

| 原文位置/章节 | 当前源码/caller | 状态与反证 |
|---|---|---|
| performance/status:1–5 总状态/已读范围 | 以下四动作都有对象和生产caller | “4/4实施”是重构动作完成，不是所有历史缺口已解决或真实性能验收通过。 |
| performance/status:9 hosts-N07 ProcessSampleRun | `scripts/simulation/escrow-performance-harness.mjs:325`对象、`:418`async facade、`:492,497`默认采样生产调用 | ownership改造实在；原文明示sampler rejection cleanup仍未修，候选C65-1。 |
| performance/status:10 hosts-R2-N18 MarketUiReportRun | `scripts/performance/market-ui-report.mjs:234`对象、`:326`main实例、`:413`finally close | 接线存在；已有C48-1启动选择/C48-2deadline与cleanup，未重复登记。Chrome未运行是验收债。 |
| performance/status:11 hosts-R2-N21 PerformanceComparisonRun | harness.mjs:481对象、:492私有measureSide、:507私有append、:511 measure、:521 buildReport、:565复用契约 | owner/samples/config快照/failed均有实际生产调用；不按旧外部可追加问题重复报漏。 |
| performance/status:12 hosts-R2-N22 Resource/EnvelopeConservation | `escrow-verification-contracts.mjs:241` Resource、`:285`EnvelopeConservation、`:373,391`verifyConservationSnapshot逐行创建 | 资源对象进入实际验证入口；无BigInt上界是获批原接受集合，不能移植engine i64/u64限制。 |
| performance/status:14–29 验证证据 | 对应三个.test.mjs现存 | 历史精准短测有明确case与10秒门禁；没有本轮重跑，未报告当前35/35。 |
| performance/status:31–35 语义/未执行 | 金额分/数量股、局部receipt守恒；harness.mjs:146读取Linux/proc、:203汇总状态、:547吞吐来源 | 保留OS runnable≠Rayon容量、吞吐≠CPU占用口径；全量/矩阵未跑不算代码缺失。 |
| performance/status:38–50 三项独立修复 | harness.mjs:492/507私有、:508独立复制sample、:533 structuredClone报告、:418 async facade | P2-01/P2-02/P3-01当前都有实现；test.mjs:445拒外部append、:453独立report、async错误用例存在。 |
| performance/status:53–55 最终复审 | 同对象与测试源码 | “无未修复有效发现”限本批已提出owner问题，不撤销原文9的既有child清理问题。 |
| performance/status:57–59 Root精准验证 | 原文明确未跑全suite/真实matrix/E2E | 不能用build07/check08核销未实施生产能力，也不能把未选case算不存在。 |
| review-account:3–9 范围证据 | 四测试文件当前getter/factory真实使用 | 静态caller迁移检查，不代Account全业务验收。 |
| review-account:11–15 三门禁 | `src/account.rs:326`原子结算、现行Position事实模型 | 基本getter迁移存在；没有由此新增制度变更、DTO字段私有化需求。 |
| review-account:17–25 重点核对 | `tests/account.rs:253`grant_position极值fixture、`src/account.rs:258`grant_position、`:352`qty checked add；strategy_state.rs:66 getter；company opening与经验对比保留 | 旧行号变化不算未实现；零投入极值fixture用于失败边界，不是生产补钱或委托旁路。 |
| review-account:27–30 发现关闭/范围 | `src/session/account_book.rs`存在，`tests/account_book.rs`不存在 | 路径限定记录已修；Account/Position生产业务问题不能由此四测试APPROVE核销。 |
| review-account:32–39 SHA | 历史审查状态绑定 | 不是当前测试结果；没有新实现要求。 |
| review-actors:1–7 总结 | 两宿主Pacing/ActorHarness现存 | 当时待root构建属于过程记录；后续status59不声称API doctest已跑。本轮未运行。 |
| review-actors:9–24 范围依据 | 两actor、fatal/protocol套件与routes订阅caller | 使用正式A股/ADR边界；未重新访问官方材料不等于重构代码缺失。 |
| review-actors:26–34 语义/屏障/Queued | desktop actor.rs:919 emit后:921pause；server actor.rs:1191broadcast后:1193pause | CivilUpdate先发再pause已实现；CommandQueued仅入队是现状，最终处理回执缺口由已有sweep01候选覆盖，不重复。 |
| review-actors:36–45 owner/范围 | DesktopPacing actor.rs:142、ServerPacing actor.rs:308；私有SessionHandles sender与subscribe方法 | Pacing实际持有running/fastest/requested_speed/meter，无第二套独立会话状态。cfg(test)Harness不冒充生产owner。 |
| review-actors:47–64 速度/恢复/fatal/订阅矩阵 | Desktop apply_speed:165、set_running:183、civil:190、fatal:196；Server :331、345、352、358；routes.rs:1271/1275、1416/1418先订阅后baseline | 所列状态转换有生产caller；Desktop非法内部值保留原日志忽略，不属于本批新增缺口；公开速度校验仍在边界。 |
| review-actors:65 doctest | 两宿主公开subscribe_events例子与compile_fail隐私验证在源码 | 测试存在、未在本批执行；不登记未实现API或缺测试代码。 |
| review-actors:67–71 有限检查/后续运行 | 后续root精准状态没有声称执行所有doctests/suites | 对验证完成度保持原边界，非新增代码任务。 |

## 新候选C65-1：性能采样失败未停止所拥有的 child

原文`performance/status.md:9`直接登记“sampler rejection 的既有 child 清理问题未修复”。本批仅对象重构无扩大行为的约束不能替代现行`docs/testing.md:99`与根AGENTS的长验证异常收尾/进程树deadline要求。

实际入口：`escrow-performance-harness.mjs:676`main→`:597`runPerformanceHarness→`:511`PerformanceComparisonRun.measure→`:497`runSample默认`:418`runProcessSample→`:383`ProcessSampleRun.start。`:385`spawn child，`:391`立刻启动异步sampler；`:395`先等待child close，之后`:400`才await sampler。`sampleTree:375`await Linux/proc或注入probe，非ENOENT/ESRCH错误由`:191–193`抛出；没有紧接的catch、finally、child kill或统一abort。采样先失败时，不会主动停止仍在运行的被测child；在child close之前该sampler Promise也没有rejection handler。Node默认未处理rejection可先终止harness，child不由此保证终止，不能据status称资源owner即完成资源生命周期。

反证：正常close在`:397`调用stopSampling，`:363`确实清timer并释放等待；spawn ENOENT最终由`:405`抛出；`:501`设置failed并禁止buildReport冒充PASS；零RSS/nonzero exit显式失败。这些覆盖正常结束/失败报告，未覆盖采样中途抛错的child收敛。Linux/proc的自然退出ENOENT/ESRCH有明确忽略分支，不把该正常race算错误。

本轮不运行真实matrix或构造长寿命子进程。缺口应限“采样失败必须受现行总deadline约束并终止owned进程树、等待清理”，不要求恢复旧benchmark、增加重试或改变吞吐/RSS判定。可与C48-2按共同工具收尾要求收口，但具体文件与producer不同；请父任务独立去重与复核。

## 排除与前批证据修正

未新增Account getter、actors owner、BigInt范围、doctest执行等缺口。API私有字段撤回是当前已批准兼容变化，不要求恢复raw sender写权限。G19固定倍率UI聚合、G39并发验收排序、Q05正式scripts测试范围继续保留，不被本批APPROVE核销。

按父任务独立复核更新`sweep14.md`的C14-2：确定缺口只依赖`tauri-host.ts:175`的stop_session IPC Promise未处理；两个unlisten静态类型为()=>void，不再无依据称三项全是Promise。独立review确认本地@tauri-apps/api 2.11.1 event.js:81返回async unlisten，仅按版本限定记录runtime观察。审计工作树无安装node_modules，本worker未冒称独立读取该本地依赖。

本轮没有新运行测试，没有改变A股单位、交易时序、T+1、经营或存档契约。
