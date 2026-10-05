# 历史需求到生产代码的实现审计（2026-10-02）

## 1. 基线、范围与判定

本报告核对历史需求与生产实现，补充并优先于 [旧缺口盘点](../../docs/implementation-gaps.md) 的完成度描述；保留旧证据，不把 A01–A11 整项重开，也不将审计视为实现授权。

静态审计固定产品基线为 `c0ab429`；当时按用户要求在新建的 `.worktree/implementation-audit-final`、`codex/implementation-audit-final` 分支续核并实施，不等待也不带入主工作区正在修改的未提交产品或正式文档。`c0ab429`仅在`43b1aa5`上更新审计，其产品代码与此前基线相同；后续已按用户要求合入 `main` 并在 `main` 继续实施，实现状态以各 G／Q 对应修复、提交及独立复核为准，不把静态基线说成当前产品。原1110来源与各阶段记录保留自己的内容指纹；此前已追完恢复边界，不把旧PASS或HEAD相同当作所有工作树当前字节相同，也不将这份历史静态审计说成覆盖后续所有并行改动。 当前主工作区已合入 UI 基线 `2c3b3bb` 及后续更新至 `cd595607`，精确恢复573文件暂存工作并解决25个冲突，保留 feature-layout、双端 Quick、精确Money／日期、member／archive／history接线；这些实施增量仍在工作树，尚未形成完整 cohesive commit。旧静态来源基线与既有短测保留各自时点，不能外推合并后全部字节已经验证。

此前234个来源路径包含229个跟踪Markdown、3份历史草稿及2份删除文档最后版本；其指纹在当前工作区未变，但这个集合不完整。使用包含 Git 忽略文件的清点后，又找到 `agents/oop-refactor-audit/` 下857个来源路径（681份不同正文），另有1份遗漏的 build 状态记录；`git log --all` 还发现18份仅在历史分支可达的文档。扩展集合共1110个来源路径、933份不同SHA正文，逐路径指纹、正文别名和批次映射见 [补扫路径清单](hidden-review/path-index.json) 与 [历史版本清单](hidden-review/history-plan.json)。新增227批、6批历史记录及1份build状态均已完成逐源EOF与指纹校验，待补读、缺少凭证及指纹不符均为0；统一映射见 [完整来源索引](hidden-review/expanded-source-index.json)。这个完成范围不包括未入集合的外部来源或未知程序缺陷。

按用户指定，扫描任务使用独立 `gpt-6-luna medium`，每个只负责1–3篇来源，连续全文读至EOF；截断处补读，完全同SHA正文共享阅读但保留全部路径。原234份已映射80份阅读记录；新增681份正文分为227批，历史分支另分6批。曾尝试50个reader并发，遇到429与文件描述符耗尽，失败批次保留待读并由原reader重试，服务恢复后错峰增加并发，不以换模型或截取摘要绕过全文要求。每章承诺归入生产已接、现行缺口、待定契约、未来/取代、文档漂移或验收证据边界，并反查当前caller、state owner与consumer。第2、3节给出已裁定状态；逐篇证据见 [覆盖清单](coverage-index.md)、[原来源清单](exhaustive-review/source-index.json) 及 [原裁定记录](exhaustive-review/resolution.md)。旧R/S/H记录保留自己的基线，不改写历史验收。

“生产已接”仅针对所列契约，不保证整模块无缺陷。确认缺口、待定需求、未来范围、文档漂移与验收证据分别登记，不用未勾选框、旧符号消失或纯函数测试证明生产功能缺失或完成。静态审计阶段没有修改游戏代码，只做源码差异与文档静态核对，未运行游戏测试、构建、浏览器、完整回归、性能矩阵或发布流程。后续先在该 worktree 实施 G 项，再合入 `main` 继续 Q 项及用户授权的未来能力，逐项状态和短测证据写入对应行，批次记录见 [补缺实施](../implementation-gap-implementation/README.md) 与 [剩余问题实施](../remaining-questions-and-features/)；不改写此前静态审计的验证范围。此前发布脚本与工作流契约的4个定向短测文件通过，属于上一基线复核的结果，静态审计没有重跑，不将其他任务的验收记录写成本次通过。 当前实施以用户最新确认的简单定向单元测试与非作者 subagent 复核为完成验证范围，不执行复杂回归；全宿主 E2E、完整回归与 Windows／macOS 实机没有执行的部分仅登记为验证限制，不作为另行阻止代码任务完成的必要门禁。实际未接生产 caller、stub、严格契约／存档事实遗漏仍须补齐，所有未来功能范围保持不变。

本次未重新联网核验制度，沿用文档中登记的 A 股规则及简化；金额为分、数量为股，界面手数仅作换算。DCF和个体策略参数是游戏模型，不冒充交易制度或真实市场校准。本批完整diff已由非作者独立复核，三项门禁通过，见 [覆盖与复核记录](coverage-index.md#独立复核记录)。这是指定基线和文档集合的静态审计，不是程序没有未知缺陷的保证。

### 最新决定优先

Q14 Simple 当前源码与验证状态以后文第3节对应条目及[公司系统实施清单](../company-system/implementation-checklist.md)、[Session接线记录](../company-system/session-integration.md)、[最终复核](../company-system/decision-sync-final-review.md)为准；当前HEAD为`04d3c49e`，其Simple代码基线为`a001681d`，早期host51／host55和下方旧Q14／D01行均为历史快照，已由本段状态取代。周期算法、四类财务／披露、Session严格当前契约、native hosts、Web分别由 `8d07a779`、`e488cbfe`、`ce5b3a1c`、`f1bc6f84`、`a001681d`提交。required cfg kind→issuer→finance/chart链已接；period为Monthly／Quarter／HalfYear／Annual，年化定点half-even，四类noise独立。seed preset可编辑，仅Day 0；price/share与明确虚拟P/E、P/B可反推，不形成后续anchor或涨跌保证。无SyntheticFunding、公司实际资金追踪、旧schema兼容或migration。股本cash cap=false实际caller未接，公司行为偏好仍缺；Q14与D01不整体核销，Simulation由用户另分支且未来范围保留。验证包括root 70 Rust、typegen 128、Web 45、root 76五个tiny case、root 75 all-targets/WASM/Vite构建、root 77 Panel 7项与tsc；最新root78 Web types、Vite 6.88秒及Release WASM check也通过。完整回归和Windows／macOS runtime未执行。旧SessionGroup/shock integration归档futureSim门禁，不算Simple通过。

当前状态补记：HEAD`04d3c49e`，Simple代码基线`a001681d`；Q14实现和验证状态详见其现位置。D01税务模块当前11项绿并经复核，早先7项失败已修复，但模块仍为低层且无生产caller；不据此宣称股息税或分红生产闭环。Simple的cash cap=false实际caller未接，公司行为偏好未完；分红仍须检查可分配利润／方案，认购核投资者本人真实现金，回购依真实委托成交。Q14／D01均未整体核销。

ADR-0023：开局前虚拟历史，内部 day=0 起实际撮合，不使用真实行情校准。
ADR-0024：现金池可以减少，不补钱、返费或保证成交。
ADR-0025：仅成功自然日日结保存，启动/明确换档才读档；内部候选回滚不是公共日内存档。
ADR-0026：机构个人阈值、真实经历、暂停买入和本人观察恢复，不自动强卖。
ADR-0019：当前单局重点，不恢复任意订单条数配额。
ADR-0027/0028：运行时宿主选择、无 Node 部署、七个手动入口、有效标签 Release/Pages；普通 commit/PR 不自动 CI，签名暂不做。2026-10-03最新决定进一步明确发布仅构建、打包、核验和部署；CI仅保留为手动开发诊断，发布及产品构建不运行测试、lint或smoke。
ADR-0018 整体仍为 proposed；只有已被后续接受的具体实施目标才列为现行缺口。

## 2. 已确认的现行缺口

静态审计确认79项：原G01–G39除第5节核销的G27外有38项，此后G40–G68共29项、G69–G79共11项，Q19纠正恢复可达性判断后转G80。实施阶段79项均已按对应确认范围补齐，确认G剩余0项；以下保留编号、原需求与证据，并在对应行登记修复，不把历史发现删除。定向短测与非作者复核不等于完整回归、浏览器/跨平台/长矩阵验收或没有未知程序缺陷，最终跨层范围见 [整合复核](../implementation-gap-implementation/final-integration-review.md)。G78包含同一恢复owner的未来待办日期边界；BeliefBook双profile身份已按Q25核实允许认知／执行分离，不虚构必需全等契约。分类及拒绝项见 [扩展裁定](hidden-review/README.md) 与 [固定基线续核](renewed-check/README.md)。静态基线当时的未来产品、仅未运行验证和主工作区半成品迁移不计入这79个G；后续授权的Q项与未来能力分别在第3、6节推进，不能用G剩余0项宣称全部任务已完成。
“未接线”指模块/类型可能已有，但生产路径没有完成承诺；“行为错误”不能靠补一个空接口解决。

### 2.1 宿主与远程链路

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G01 | 已补齐：浏览器远程 HTTP/WS 可用且鉴权；ADR-0005 §6、ADR-0027 | 浏览器WS通过认证subprotocol携凭据，Server握手解析并保留会话鉴权，HTTP保持Bearer；不把凭据放URL或开放私有路由。 Remote 53项、heartbeat 5项与WS 10项短测通过，非作者完整diff再审通过；未跑真实浏览器网络/公网验收。见 [实施复核](../implementation-gap-implementation/remote-chain-review.md)；原证据见 [宿主](reaudit-host.md)。 |
| G02 | 已补齐：三宿主实际倍率；ADR-0005 §5、UX-CONTRACT 模拟控制 | Remote实际倍率请求附认证凭据，沿现有UI轮询消费真实宿主指标。 Remote 53项、heartbeat 5项与WS 10项短测通过，非作者完整diff再审通过；未跑真实浏览器网络/公网验收。见 [实施复核](../implementation-gap-implementation/remote-chain-review.md)；原证据见 [宿主](reaudit-host.md)。 |
| G03 | 已补齐：remote push/pull 都可持续取帧；ADR-0005 §6、ADR-0010 | Remote pull真实发送GetFrame，单inflight并等协议响应后继续，deadline与连接切换清理请求，不无界积压。 Remote 53项、heartbeat 5项与WS 10项短测通过，非作者完整diff再审通过；未跑真实浏览器网络/公网验收。见 [实施复核](../implementation-gap-implementation/remote-chain-review.md)；原证据见 [宿主](reaudit-host.md)。 |
| G04 | 已补齐：baseline 仅初始化、读档、显式重同步；ADR-0010 §统一更新 | Remote/Tauri正常暂停继续不重送旧baseline；仅首次交付、读档及显式重同步建立基线，拒绝交付不标记完成，显式重试可重送。 Node相关短测与Rust actor 15项短测通过、非作者再审通过；未跑浏览器长验收。另见 [Remote应用链](../implementation-gap-implementation/remote-chain.md)。见 [实施复核](../implementation-gap-implementation/host-controls-review.md)；原证据见 [宿主](reaudit-host.md)。 |
| G05 | 已补齐：心跳失活处理与重连；ADR-0005 §6、ADR-0010 宿主能力 | Server独立监督匹配Pong期限，阻塞await亦到期释放连接，迟到Pong与后续Ping不续旧期限；Remote有限自动恢复与旧连接身份隔离，不重发委托。 Remote 53项、heartbeat 5项与WS 10项短测通过，非作者完整diff再审通过；未跑真实浏览器网络/公网验收。见 [实施复核](../implementation-gap-implementation/remote-chain-review.md)；原证据见 [宿主](reaudit-host.md)。 |
| G18 | 已补齐：Worker uiFrame 背压；ADR-0010 宿主能力/不做 | Worker以消费者接纳后的uiFrame ACK实行有界credit及16ms目标批次，完整提交/交易事实保留；ACK不等React绘制。拒绝或fatal终态停止消费，不错误ACK后续帧。 Node相关短测与Rust actor 15项短测通过、非作者再审通过；未跑浏览器长验收。见 [实施复核](../implementation-gap-implementation/host-controls-review.md)；原证据见 [宿主](reaudit-host.md)。 |
| G19 | 已补齐：Tauri 固定高倍率 tick 在 Rust 聚合后约 16ms 发布；ADR-0010 | Desktop固定倍率按现实到期tick债务在Rust聚合后约16ms交付，只扣真实完成tick，CPU预算保留尾数，冷启动/双窗口/完整日序列覆盖。 Node相关短测与Rust actor 15项短测通过、非作者再审通过；未跑浏览器长验收。见 [实施复核](../implementation-gap-implementation/host-controls-review.md)；原证据见 [宿主](reaudit-host.md)。 |
| G40 | 已补齐：三宿主有副作用控制命令等待宿主确认；ADR-0010:60 | 三宿主start/stop/setSpeed等待实际应用确认：Worker applied回执、Desktop actor oneshot、Remote actor→HTTP Promise；UI与偏好storage确认后才更新。CommandQueued仍仅订单入队。 Node相关短测与Rust actor 15项短测通过、非作者再审通过；未跑浏览器长验收。另见 [Remote应用链](../implementation-gap-implementation/remote-chain.md)。见 [实施复核](../implementation-gap-implementation/host-controls-review.md)；原证据见 [宿主全文](exhaustive-review/luna01.md) 与 [actor](exhaustive-review/luna62.md)。 |
| G53 | 已补齐：恢复响应必须推进generation；历史task30:26–27 | restoreWorkerSlot先验证nextGeneration为正安全整数且严格大于原generation，再交付snapshot；短测覆盖旧/同代拒绝、推进成功及零/负数/小数/不安全整数/字符串，错误保留actual/expected代次。 原证据见 [Worker](exhaustive-review/luna29.md)，实施与短测见 [前端边界](../implementation-gap-implementation/web-boundaries.md)。 |
| G66 | 已补齐：已接受Remote请求保留完成或显式错误出口；ADR-0010、错误处理原则 | Remote HTTP、写请求、刷新与读档waiter在超时/dispose/重同步/断连都有明确终态，并隔离迟到确认；中断写请求只说明结果未知，不假称未执行。 Remote 53项、heartbeat 5项与WS 10项短测通过，非作者完整diff再审通过；未跑真实浏览器网络/公网验收。见 [实施复核](../implementation-gap-implementation/remote-chain-review.md)；原证据见 [Remote](exhaustive-review/luna60.md) 与 [调用](exhaustive-review/sweep61.md)。 |

### 2.2 策略、个人信息与估值

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G06 | 已补齐：五年权益现金流折现；公司计划 K5/K5a，`docs/superpowers/plans/2026-09-10-company-information-npc-intentions.md:150` | FCFE每期按对应年数折现，终值仍折现5年；逐步整数半偶到分。恒定FCFE=17,820,000分、r=10%、g=gt=0，独立逐期推导及修复结果均为178,200,000分；悲观/乐观与真实披露→个人信念gold同核。原证据见[策略与公司](reaudit-engine.md)，修复短测与独立推导见 [模型边界](../implementation-gap-implementation/model-boundaries.md)。 |
| G07 | 已补齐：身份不决定分析能力，散户可分析基本面及五路权重；公司计划 K5、任务17/26，ADR-0016 | Retail装配个人AnalysisProfile/BeliefBook，DecisionSnapshot捕获仅获知本人材料并封存五路判断，DecisionShadow实际消费混合信号；保留ZiNoise复杂度、真实随机到达、T+1和风险优先，不设置Institution policy。 Retail 72/72及真实恢复边界短测通过，非作者再审通过；早期TDD行为红证据不足如实登记，不补写历史。见 [实施复核](../implementation-gap-implementation/retail-beliefs-review.md)；原证据见 [策略与公司](reaudit-engine.md)。 |
| G08 | 已补齐：失败日期与每20交易日无新受挫衰减；ADR-0013 计划契约修订、公司计划 K5 | 散户开局观察及真实成交进入dated writer，DecisionSnapshot捕获封存每20交易日无新受挫的派生影响，DecisionShadow消费衰减但不删除历史失败；真实买入和初始分配区分，机构不重复衰减。 capture 18/18、settlement 24/24相关短测通过，非作者完整diff再审通过。见 [实施复核](../implementation-gap-implementation/personal-strategy-review.md)；原证据见 [策略与公司](reaudit-engine.md)。 |
| G09 | 已补齐：本人已知完整年报及可用中期更新；公司计划 K5a:148 | 本人最新年报及中期进入真实Institution/Retail root；中期同比仅修订同scope全年基准，不年化或混口径。同期间本人已知Consolidated优先是明确游戏政策，缺归母NI拒绝、缺可靠归母CF的DCF显式不可用。 fundamental 26/26及真实双root scope短测通过，官方全文独立核验与非作者再审通过；不冒称交易所强制估值规则。见 [实施复核](../implementation-gap-implementation/personal-strategy-review.md)；原证据见 [策略与公司](reaudit-engine.md)。 |
| G20 | 已补齐：新局从熵取种、测试可固定；ADR-0005 §4 | 普通无档新局使用Web Crypto完整u64熵，允许0，熵不可用显错而不创建宿主；E2E固定种子、读档种子无损独立。 helper与真实lifecycle短测通过，非作者完整diff复核通过；不要求自由并发整局字节一致。见 [实施复核](../implementation-gap-implementation/seed-baseline-review.md)；原证据见 [宿主](reaudit-host.md)。 |
| G42 | 已补齐：个人价格记忆按持仓∪活跃计划＋8修剪，恢复验证同一边界；K5、任务19/25 | 持仓∪真实active计划保护其余最多8条，Root/Lifecycle后预算、提交候选内按真实receipt变更账户修剪并在成功提交时发布，成功日结再收口，restore同一界限；不每tick扫描全户，不以全市场股票数冒充边界。 实际9条拒绝、小fixture完整往返、8条/active保护接受guard及真实清仓收口短测通过，非作者再审通过；9股完整接受restore后续装配未测。见 [实施复核](../implementation-gap-implementation/personal-strategy-review.md)；原证据见 [个人记忆](exhaustive-review/luna21.md) 与 [计划](exhaustive-review/luna03.md)。 |
| G43 | 已补齐：淡出股票不再自动获知/分析/建立新计划；K6候选范围、任务25 | 候选仅来自持仓、关注、active计划及本人重新发现，历史belief条目不自动入候选；淡出后旧信息/经历保留，不自动获新消息或新建计划。 真实 root 淡出后不获知／不建计划短测及非作者复核通过；Q11 的日终订阅曾重新引入历史 belief 兜底，已在 `015a152` 去除，代表性 case 的淡出／持仓／关注／活动计划四分支与完整 save／restore 均通过，真实红测0.20秒、绿测1.62秒及非作者复核通过，见[订阅修复](../remaining-questions-and-features/q11-faded-subscriptions.md)；不替Q02主动读公开历史定约。见 [实施复核](../implementation-gap-implementation/personal-strategy-review.md)；原证据见 [生产root](exhaustive-review/luna03.md)。 |
| G69 | 已补齐：计划恢复保持非零且可表示的期限；输入校验原则、历史strategy D02 | 开户、TradingPlan serde、PlanBook与完整Session validator共用期限校验；先减一再求最后有效日，MAX/1合法，零期限及越界显式拒绝。新增直接API及完整SaveSlot定向短测，不改变A股订单有效期。原问题见 [期限原文复核](hidden-review/batch-068.md) 与 [裁定](hidden-review/candidate-resolution-01.md)；实施证据见 [恢复批](../implementation-gap-implementation/restore-guards.md)。 |

### 2.3 行情显示与日历边界

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G10 | 已补齐：连续竞价一分钟成交量；DESIGN:88、UX-CONTRACT:47 | 连续量按权威累计量差累加到分钟，竞价完成量建立连续量基线，股与界面手数不混。 53项定向短测及双文件SSR通过，非作者再审通过；未做浏览器像素或性能矩阵。见 [实施复核](../implementation-gap-implementation/chart-stream-review.md)；原证据见 [界面](reaudit-ui.md)。 |
| G11 | 已补齐：新日清旧分时且保留新日已到采样；UX-CONTRACT:81 | 同批次跨日清旧缓存并保留新日采样；BeforeOpen屏障清旧日分时与竞价，AfterClose-only保留真实收盘图与日K。 53项定向短测及双文件SSR通过，非作者再审通过；未做浏览器像素或性能矩阵。见 [实施复核](../implementation-gap-implementation/chart-stream-review.md)；原证据见 [界面](reaudit-ui.md)。 |
| G12 | 已补齐：分时量涨红空心、跌绿实心；UX-CONTRACT:54 | 按前有效价格判涨跌，同竞价槽不以自身作前价；真实SVG量柱涨红空心、跌绿实心，不伪装主动买卖方向。 53项定向短测及双文件SSR通过，非作者再审通过；未做浏览器像素或性能矩阵。见 [实施复核](../implementation-gap-implementation/chart-stream-review.md)；原证据见 [界面](reaudit-ui.md)。 |
| G13 | 已补齐：逐笔展示最近成交；DESIGN:126、移动QA | 实际最新优先成交带取slice(0,7)，不反转或无界积累。100笔成交短Fixture验证显示100至94，原数组不修改；仍保留三宿主最近100条边界。 原证据见 [界面](reaudit-ui.md)，实施与短测见 [前端边界](../implementation-gap-implementation/web-boundaries.md)。 |
| G14 | 已补齐：逐笔时间对应真实成交；DESIGN:126 | 逐笔从所属提交帧tick经effect与Redux送达详情，逐行显示自身时间；缺字段明确提示，不借当前时钟。 53项定向短测及双文件SSR通过，非作者再审通过；未做浏览器像素或性能矩阵。见 [实施复核](../implementation-gap-implementation/chart-stream-review.md)；原证据见 [界面](reaudit-ui.md)。 |
| G15 | 已补齐：官方年度覆盖替代模拟回退；`docs/simulation-calendar.md:59` | 对存在Official覆盖的交易所/年度，非周末且不在覆盖休市区间的日期为Trading，不叠加模拟假日；其他交易所/年度保留fallback，周末不开放。合成Fixture短测覆盖上述边界，默认空official表不变。原证据见[基础](reaudit-foundations.md)，验证见 [模型边界](../implementation-gap-implementation/model-boundaries.md)。 |
| G75 | 已补齐：日内时刻恢复与构造器使用同一秒域；`CivilInstant`输入契约 | serde经过CivilInstant::new校验，0与86399合法，86400及u32::MAX拒绝；公开秒域不再可由私有字段派生serde绕过。原问题见 [原文复核](hidden-review/batch-139.md) 与 [裁定](hidden-review/candidate-resolution-04.md)；短测见 [恢复批](../implementation-gap-implementation/restore-guards.md)。 |
| G76 | 已补齐：冻结日历政策内容身份包含官方出处摘要；政策来源绑定契约 | policy content digest新增独立source_digest分量；只修改出处摘要即改变身份，旧digest与新摘要组合在validate拒绝。默认official空表身份不变；测试使用合成出处，不使用真实行情。原问题见 [来源审读](hidden-review/batch-156.md) 与 [裁定](hidden-review/candidate-resolution-04.md)；短测见 [恢复批](../implementation-gap-implementation/restore-guards.md)。 |
| G44 | 已补齐：零成交量如实为零；DESIGN:88/110、移动QA:3 | 分时与K线投影在volume=0时保留槽位及零高度，只有正量使用最小可见高度1。短测覆盖零/微小正量/最大量，真实DOM与SVG消费者无额外最小高度；不伪造有成交。 原证据见 [绘图](exhaustive-review/luna02.md)，实施与短测见 [前端边界](../implementation-gap-implementation/web-boundaries.md)。 |
| G46 | 已补齐：桌面五档标签对应真实报价rank；DESIGN、UX盘口 | 真实Desktop卖盘先按engine报价索引标rank=index+1，再反转显示，卖一始终邻近买一；一档/两档/五档SSR核对标签与报价，不伪造空档。 原证据见 [盘口](exhaustive-review/luna02.md)，验证见 [资产与盘口](../implementation-gap-implementation/portfolio-and-book.md)。 |
| G47 | 已补齐：竞价null指示价槽不绘价格线；DESIGN:88、UX:50 | MobileIntradayProjection按null竞价槽分割连续有效片段，真实SVG逐段绘价格线，孤立有效点仍绘dot；空/全null不绘价，所有量槽保留。原证据见 [竞价绘图](exhaustive-review/luna02.md)，短测与复核见 [竞价片段](../implementation-gap-implementation/auction-segments.md)。 |

### 2.4 工程、交互和发布

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G16 | 已补齐：普通tick不随多年历史线性复制；`docs/superpowers/plans/2026-09-24-single-world-multithreading.md:60` | PlanBook固定radix COW共享历史，借用序列化保持原JSON；ClosingEngine/PublicLibrary以Arc在shadow/root共享，真实写入make_mut，保存提取权威事实，不删除历史或擅行全套WAL。 共享/修改隔离、JSON字节兼容及14项hash短测通过，非作者复核通过；未跑多年tick吞吐矩阵。见 [实施复核](../implementation-gap-implementation/tick-performance-review.md)；原证据见 [当前历史owner](exhaustive-review/luna10.md) 与 [策略](reaudit-engine.md)。 |
| G17 | 已补齐：Rust指标与Rayon生产加速；ADR-0008 D2 | 测量小输入并行更慢后保留顺序，2048及以上样本在统一生产单项入口以Rayon join并行独立指标；三宿主真实调用、精确数值与nightly WASM编译通过。 短测、真实WASM target编译与非作者复核通过；未跑长期吞吐矩阵。见 [实施复核](../implementation-gap-implementation/tick-performance-review.md)；原证据见 [基础](reaudit-foundations.md)。 |
| G21 | 已补齐：基线CLI只取setup、不验证其余档字段；`docs/price-volume-simulation-gap-checklist.md:265` | CLI仅反序列化setup投影并执行启动配置校验，无关schema/runtime/envelope字段不影响新局；公共SaveSlot深校验不放宽。5项定向Rust短测通过且由非作者独立重跑，完整diff复核通过，未跑长模拟。见 [实施复核](../implementation-gap-implementation/seed-baseline-review.md)；原证据见 [工具](reaudit-tools.md)。 |
| G22 | 已补齐：表单即时/字段级错误；`docs/error-handling.md:114` | 真实交易hook即时生成价格/数量字段错误，App委托与条件单用aria-invalid/aria-describedby关联；买卖与零股卖出仍按原规则预检。 定向短测、真实consumer完整diff独立复核通过；浏览器焦点、视觉、辅助技术矩阵未执行。见 [实施复核](../implementation-gap-implementation/ui-contracts-review.md)；原证据见 [界面](reaudit-ui.md)。 |
| G23 | 已补齐：进入详情聚焦返回、返回聚焦原列表；UX-CONTRACT Flow ledger | 详情挂载后聚焦返回；返回恢复原股票行，失联时聚焦行情区域，preventScroll保持滚动；切股不抢焦点。 定向短测、真实consumer完整diff独立复核通过；浏览器焦点、视觉、辅助技术矩阵未执行。见 [实施复核](../implementation-gap-implementation/ui-contracts-review.md)；原证据见 [界面](reaudit-ui.md)。 |
| G24 | 已补齐：信息标签切换保持滚动；UX-CONTRACT:69 | 真实信息tab handler只更新状态，移除主动scrollIntoView。 定向短测、真实consumer完整diff独立复核通过；浏览器焦点、视觉、辅助技术矩阵未执行。见 [实施复核](../implementation-gap-implementation/ui-contracts-review.md)；原证据见 [界面](reaudit-ui.md)。 |
| G25 | 已补齐：返回/切股至少44px点击热区；DESIGN:86 | 最终CSS返回首列44px，返回及切股热区至少44px，图标可独立缩放。 定向短测、真实consumer完整diff独立复核通过；浏览器焦点、视觉、辅助技术矩阵未执行。见 [实施复核](../implementation-gap-implementation/ui-contracts-review.md)；原证据见 [界面](reaudit-ui.md)。 |
| G26 | 已补齐：手动开发CI的前端warning作为错误；`docs/tech-stack.md:23` | Web lint命令添加--deny-warnings，手动开发CI沿用该入口；实际两线程oxlint短Fixture对照原warning退出0、新命令warning退出1、合法代码退出0，不把lint加入发布。 原证据见 [工具](reaudit-tools.md)，短测见 [CI与Pages边界](../implementation-gap-implementation/ci-pages.md)。 |
| G45 | 已补齐：自选详情返回原列表身份；UX导航/Flow ledger | reducer保留原primaryTab；App使用共用MobileDetailLayer按detailCode显示详情，不再仅允许market页。短测覆盖自选→详情→切股→交易底页→返回，并实际SSR共用详情层，返回仍在自选。 原证据见 [导航](exhaustive-review/luna02.md)，实施与短测见 [前端边界](../implementation-gap-implementation/web-boundaries.md)。 |
| G48 | 已补齐：中文页面语言与辅助文本一致；UX:8/10、ADR-0007 | HTML语言为zh-CN，真实AG Grid消费已核安装版本key的中文locale与辅助文本。 定向短测、真实consumer完整diff独立复核通过；浏览器焦点、视觉、辅助技术矩阵未执行。见 [实施复核](../implementation-gap-implementation/ui-contracts-review.md)；原证据见 [页面](exhaustive-review/luna02.md) 与 [Grid](exhaustive-review/sweep41.md)。 |
| G49 | 已补齐：持仓成本及浮盈承接半偶到分语义；account spec:23/63 | 持仓展示共用valueHeldPosition，BigInt中间计算正负对称半偶到每股分，浮盈=(现价−舍入成本)×股数，与Rust Account一致；真实SSR验证200股净投入±200100分的成本与浮盈，单位/费用/T+1不变。 原证据见 [账户消费](exhaustive-review/luna15.md)，验证见 [资产与盘口](../implementation-gap-implementation/portfolio-and-book.md)。 |
| G50 | 已补齐：React渲染异常进入可见详情/反馈出口；错误处理规范 | `RenderErrorBoundary` 在真实root包裹Provider/App，React render/layout失败复用脱敏、复制反馈与刷新警告；安全读取message描述符，不执行访问器，非法类型显式诊断并保留cause。12项相关短测通过，非作者完整diff复核通过；未运行浏览器异常捕获验收，不泛称覆盖事件/异步异常。实现见 [React错误出口](../implementation-gap-implementation/react-errors.md)，原证据见 [错误链](exhaustive-review/luna14.md)。 |
| G51 | 已补齐：Desktop释放失败显式上报；错误处理规范 | Tauri dispose等待stop_session IPC并保留fatal出口，释放失败Promise拒绝并由真实生命周期展示，不再fire-and-forget。 Node相关短测与Rust actor 15项短测通过、非作者再审通过；未跑浏览器长验收。见 [实施复核](../implementation-gap-implementation/host-controls-review.md)；原证据见 [释放](exhaustive-review/sweep14.md)。 |
| G52 | 已补齐：协议断言保留已知actual/expected上下文；错误详情规范 | 协议失败提供真实actual/expected公开游标并经实际复制反馈保留，敏感值仍脱敏；未知类型不猜tick，callback失败使用处理前attempt cursor。 Node相关短测与Rust actor 15项短测通过、非作者再审通过；未跑浏览器长验收。三项反馈上下文发现均修复并由web_gap_review独立再审通过。见 [实施复核](../implementation-gap-implementation/host-controls-review.md)；原证据见 [协议错误](exhaustive-review/luna14.md)。 |
| G54 | 已补齐：Money公开解析拒绝完全无数字输入；Money spec/Task4 | Money::from_yuan_str显式拒绝“.”、“+.”、“-.”及带空白形态，保留“.5”“12.”等含数字的既有合法输入。影响限定公开库API，不冒称UI/存档此前已接受。原证据见 [解析](exhaustive-review/luna18.md)，短测见 [模型边界](../implementation-gap-implementation/model-boundaries.md)。 |
| G55 | 已补齐：公开Strategy构造/工厂统一拒非法参数；策略spec/防御原则 | Momentum构造器拒非有限阈值，params.validate与Factory沿用同一入口；机构原始margin在采样/钳位前校验，零tick日显式InvalidParam。新短测验证非法参数拒绝、margin/零tick不消耗RNG及合法三类实例可建。原证据见 [策略入口](exhaustive-review/sweep20.md) 与 [工厂](exhaustive-review/sweep19.md)，实施见 [策略输入](../implementation-gap-implementation/strategy-input.md)。 |
| G56 | 已补齐：Pages区分owner根站点和项目路径；ADR-0028 | Pages base仅在仓库名与repository_owner.github.io不区分大小写精确一致时为根路径；异owner同后缀为项目路径。步骤传owner并校验env形状，VM执行真实JS核root/project与非法env，不冒称线上部署验收。 原证据见 [Pages](exhaustive-review/luna12.md)，短测见 [CI与Pages边界](../implementation-gap-implementation/ci-pages.md)。 |
| G64 | 已补齐：桌面行情选股有键盘等价入口；UX/设计辅助功能 | 真实AG Grid开启cell focus，Enter/Space走同一选股入口，保留方向键导航并提供中文说明。 定向短测、真实consumer完整diff独立复核通过；浏览器焦点、视觉、辅助技术矩阵未执行。见 [实施复核](../implementation-gap-implementation/ui-contracts-review.md)；原证据见 [桌面Grid](exhaustive-review/sweep41.md)。 |
| G65 | 已补齐：主导航/行情分类选中状态程序化公开；UX辅助功能 | 真实主导航/行情分类公开aria-current；交易按钮公开展开状态与目标，不强加错误tab角色。 定向短测、真实consumer完整diff独立复核通过；浏览器焦点、视觉、辅助技术矩阵未执行。见 [实施复核](../implementation-gap-implementation/ui-contracts-review.md)；原证据见 [状态](exhaustive-review/luna02.md)。 |
| G67 | 已补齐：外部baseline持仓必须有行情，估值缺项显式失败；防御/资产契约 | 外部baseline深验所有账户持仓代码有own行情引用；portfolio selector和组件不再缺价默认为0。覆盖玩家/非玩家、零股引用及继承属性，错误保留持仓代码和协议路径；delta原guard不改。 原证据见 [资产](exhaustive-review/luna15.md)，验证见 [资产与盘口](../implementation-gap-implementation/portfolio-and-book.md)。 |
| G68 | 已补齐：合法非默认局证券使用当前setup交易规则/选项；存档编辑、交易规则一致性 | 股票选项、当前证券类别、快捷涨跌停均消费activeSetup；非默认证券无名称时显示代码，不编造名称。 定向短测、真实consumer完整diff独立复核通过；浏览器焦点、视觉、辅助技术矩阵未执行。见 [实施复核](../implementation-gap-implementation/ui-contracts-review.md)；原证据见 [表单](exhaustive-review/luna02.md) 与 [裁定](exhaustive-review/resolution.md)。 |

### 2.5 补充逐章核对发现

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G28 | 已补齐：固定集团合并报表进入完整交付；ADR-0016:97、公司计划K3/任务12–13/完成条件 | 固定groups进入setup/state/save/restore、月末合并和真实公开链；按行业科目定义区分同码异义，逐期损益有上下界。真实Journal/对手方/库存source派生往来及内部销售抵销、累计账面上界，五产物和重述拒绝保持登记边界。 真实Session 3/3、合并27/27、报表及来源短测、六真实档Web桥接通过；非作者再审通过，不核销Q17其他边界。见 [实施复核](../implementation-gap-implementation/company-assembly-review.md)；原证据见 [策略与公司](reaudit-engine.md)。 |
| G29 | 已补齐：无NPC时跳过初始流通盘分配；initial-positions spec§2.4、计划Task3 | ByKind仅在确有NPC且正流通盘需要分配时要求有效种类权重和，零NPC沿用空集合seed早退；不赠股、不放宽各权重finite/非负校验。Random/正权重及全零ByKind真实新局→tick→日结→恢复均维持玩家零持仓，已有有效种类非法权重反例保留。原证据见[基础](reaudit-foundations.md)，短测见 [零NPC](../implementation-gap-implementation/zero-npc.md)。 |
| G30 | 已补齐：图表窗口按钮可见键盘焦点；DESIGN:82、UX-CONTRACT:56 | 详情根定义msd-focus，真实图表工具按钮焦点样式消费该token。 定向短测、真实consumer完整diff独立复核通过；浏览器焦点、视觉、辅助技术矩阵未执行。见 [实施复核](../implementation-gap-implementation/ui-contracts-review.md)；原证据见 [界面](reaudit-ui.md)。 |
| G31 | 已补齐：图表只随当前股票历史刷新；UX-CONTRACT:88 | 当前股票未变时复用冻结readonly分时/竞价/日K数组，无关股票与同槽无变化不换引用。 53项定向短测及双文件SSR通过，非作者再审通过；未做浏览器像素或性能矩阵。见 [实施复核](../implementation-gap-implementation/chart-stream-review.md)；原证据见 [界面](reaudit-ui.md)。 |
| G32 | 已补齐：昨收中轴与0.00%位置一致；DESIGN:90 | SVG、昨收中轴与0%标注共享扣除时间轴后的真实绘图区，不再使用不同高度中点。 定向短测、真实consumer完整diff独立复核通过；浏览器焦点、视觉、辅助技术矩阵未执行。见 [实施复核](../implementation-gap-implementation/ui-contracts-review.md)；原证据见 [界面](reaudit-ui.md)。 |
| G33 | 已补齐：移动端字号随容器宽度响应；DESIGN:64/127 | 后置报价、摘要、周期和盘口字号改为clamp与cqw，不再用固定px覆盖容器响应式规则。 定向短测、真实consumer完整diff独立复核通过；浏览器焦点、视觉、辅助技术矩阵未执行。见 [实施复核](../implementation-gap-implementation/ui-contracts-review.md)；原证据见 [界面](reaudit-ui.md)。 |
| G34 | 已补齐：交易底页遵守减少动态效果偏好；DESIGN:104、UX-CONTRACT:95 | 最终App reduced-motion规则覆盖详情外交易底页和遮罩，取消transition/animation。 定向短测、真实consumer完整diff独立复核通过；浏览器焦点、视觉、辅助技术矩阵未执行。见 [实施复核](../implementation-gap-implementation/ui-contracts-review.md)；原证据见 [界面](reaudit-ui.md)。 |


### 2.6 公司与计划生产闭环补漏

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G35 | 已补齐：工商折旧、所得税及跨行业商业债务支付进入经营闭环；公司计划K3/任务8、14 | 真实经营caller执行月折旧、年末所得税和工商/地产债务、银行DEP本息到期支付；零额折旧推进月份而不造凭证，不足资金保留待付并重试，Session失败回滚不重复计提。 相关25项定向短测及真实年末失败回滚/重试通过，非作者再审通过；Q23直接公共税API边界不变。见 [实施复核](../implementation-gap-implementation/company-operations-review.md)；原证据见 [策略与公司](reaudit-engine.md)。 |
| G36 | 已补齐：四行业可自定义并经公共报告查询跑通；公司计划K3:104、任务26 | 显式CompanyOperationsConfig可装配四行业，默认仍Industrial；公共查询、封账、披露、save/restore闭合，行业不适用冲击不采样/注入/公布，Bank/Insurance真实适用公告有非空负控。 真实四行业Session及Bank/Insurance非空公开公告短测、真实存档Web桥接通过，非作者联合复核通过。见 [实施复核](../implementation-gap-implementation/company-assembly-review.md)；原证据见 [策略与公司](reaudit-engine.md)。 |
| G37 | 已补齐：DEV因果记录关联实际订单ID和计划变化；公司计划任务35 | DEV trace沿真实Lifecycle/Allocation/RouteOutcome关联实际Accepted/Canceled订单ID、计划变化和真实预算约束；只读查询/重复稳定与私有状态隔离保留，不把未成交伪造成交。 真实受理/取消/计划与只读trace短测、Web实际展示通过，非作者完整diff再审通过。见 [实施复核](../implementation-gap-implementation/personal-strategy-review.md)；原证据见 [策略与公司](reaudit-engine.md)。 |
| G38 | 已补齐：本人预算区分已有计划续行与新机会；公司计划K6:165/任务22 | root封存原active PlanId，经Lifecycle区分同账户已有计划续行与新机会，有限现金优先满足续行；减仓和真实冻结/费用规则保留，不改变跨账户撮合优先级。 真实有限现金grant/constraint短测通过，非作者完整diff复核通过。见 [实施复核](../implementation-gap-implementation/personal-strategy-review.md)；原证据见 [策略与公司](reaudit-engine.md)。 |
| G41 | 已补齐：真实支付失败/逾期状态持久并公开风险材料；K2:92、任务8/14 | 经营真实付款失败按日保存权威历史，18:00派发只记录公开风险公告；save恢复验证日期、公司、金额、事项及active域，不造经济冲击、不补投资者钱，日终事务覆盖历史与重试。 真实Session公共公告、save/restore及回滚/重试短测通过，非作者再审通过；Q11真正违约cause不冒称核销。见 [实施复核](../implementation-gap-implementation/company-operations-review.md)；原证据见 [日结](exhaustive-review/luna03.md)。 |
| G58 | 已补齐：每贷款人独立授信按对应未偿校验；公司商业合同 | borrow/available_credit按LoanState关联的真实合同counterparty求该lender未偿本金，包含开局债务与还本。A/B各授信1000且A用满时，B仍可借满1000；各自超额拒绝不改状态，A还款只恢复A额度。独立于G35支付调度，原证据见 [授信](exhaustive-review/luna31.md)，短测及复核见 [工商授信](../implementation-gap-implementation/industrial-credit.md)。 |
| G59 | 已补齐：有明确保障期限的保险不在期后新造事故赔案；K3/公司会计 | 经营赔案日程新增coverage_end门控，结束日及之前仍按elapsed节奏发生，期后不新增；服务释放和已发生未付赔案期后支付保留。真实CompanyOperations短Fixture验证结束日、两个期后日期及原赔案结付，不因G36会话装配未完而伪核销。原证据见 [承保期限](exhaustive-review/luna55.md)，实施见 [保险期限](../implementation-gap-implementation/insurance-coverage.md)。 |
| G70 | 已补齐：工商开局库存子账与对应总账逐科目对账；公开构造契约、历史company D01 | seed_inventory补核工业报表同真源的1403/1405/5001；存在但缺seed的科目按零子账余额对账，不平账补钱。custom chart没有对应科目不强行添加，其余seed仍逐项核。原证据见 [开局审读](hidden-review/batch-037.md) 与 [裁定](hidden-review/candidate-resolution-01.md)，短测见 [模型边界](../implementation-gap-implementation/model-boundaries.md)。 |
| G71 | 已补齐：经营调度器恢复保持唯一身份及序号耗尽显式错误；保存恢复纪律 | from_parts与serde拒绝跨日期重复ScheduledDueId，完整SaveSlot解码沿用同一守卫。submit先checked_add，MAX游标显式SequenceExhausted且队列/游标不变；MAX-1仍可分配，不混用A股委托ID。原问题见 [调度审读](hidden-review/batch-037.md) 与 [恢复裁定](hidden-review/candidate-resolution-01.md)；短测见 [恢复批](../implementation-gap-implementation/restore-guards.md)。 |
| G73 | 已补齐：保险组反序列化保持经营不变量；外部存档校验原则 | ContractGroupState serde与直接SaveSlot恢复均验证保险经营不变量、释放进度、金额及carry；正确处理可消去MAX/MIN，不缩小合法域，内部失败测试原强断言保留。 当前production树23/23短测通过，含真实内存损坏组→完整restore拒绝，三轮独立复核通过；不承诺所有保险API强事务。见 [实施复核](../implementation-gap-implementation/insurance-restore-review.md)；原证据见 [恢复裁定](hidden-review/candidate-resolution-01.md)。 |
| G74 | 已补齐：AccountValidation结果身份预检失败后coordinator按既有契约停用；typed failure状态机 | Continuous及Auction身份预检错误调用既有fail锁存；两项短测验证失败后next_ready_batch与finish均拒绝，不改变正常撮合或宣称默认交易曾错序。原证据见 [预检原文](hidden-review/batch-141.md) 与 [裁定](hidden-review/candidate-resolution-03.md)，短测见 [模型边界](../implementation-gap-implementation/model-boundaries.md)。 |
| G77 | 已补齐：合并往来金额与工作底稿保持既有恒正契约；DTO及公开消除API | precheck_balance拒绝非正声明并携成员、对手方及金额；两种申报顺序均拒零/负配对，真实AR/AP正额仍生成1条底稿，成员账套不变。独立于G28集团接线、账面上界及行业分类。原证据见 [历史对照](hidden-review/batch-175.md) 与 [正额裁定](hidden-review/candidate-resolution-05.md)，短测见 [模型边界](../implementation-gap-implementation/model-boundaries.md)。 |
| G78 | 已补齐：自然日时钟恢复保持到期业务身份、耗尽错误及日期适用范围；CivilClock保存契约 | from_parts拒绝重复DueBusinessId并按冻结policy查询每条pending日期，完整SaveSlot恢复进入同一守卫。注册先checked_add，MAX游标显式DueSequenceExhausted且零变更，空队列游标0接受集合不变；与G71是不同owner。原问题见 [原裁定](hidden-review/candidate-resolution-06.md) 与 [日期续核](renewed-check/clock-bounds.md)；短测见 [恢复批](../implementation-gap-implementation/restore-guards.md)。 |
| G79 | 已补齐：工商授信计算错误不得伪装无授信；显式错误与外部状态校验 | available_credit改Result<Option>，无额度为Ok(None)，算术错误明确Err；IndustrialBooks serde与完整SaveSlot重验本金/利息可加总、非负、合同/贷款人关联及日期/余数边界。保留零本金、半分余数端点与期后计提，不证明编辑资产的历史来源。原证据见 [公开API复核](hidden-review/batch-176.md) 与 [裁定](hidden-review/candidate-resolution-06.md)，短测及复核见 [工商授信](../implementation-gap-implementation/industrial-credit.md)。 |
| G80 | 已补齐：完整存档恢复校验银行ECL政策；外部输入及行业政策契约 | validate_company_domain对Bank账套调用EclPolicy::validate，错误携公司ID与Bank ECL上下文；完整SaveSlot短测验证合法政策可恢复、两表空值/非法权重及PD拒绝。保持库级serde接受集合及issue_loan次序，不冒称G36四行业闭环已实现。原问题见 [续核](renewed-check/bank-restore.md) 与 [独立裁定](renewed-check/independent-review.md)；实施见 [恢复批](../implementation-gap-implementation/restore-guards.md)。 |

### 2.7 验收工具契约

| ID | 要求与原文 | 生产证据、缺少环节及影响 |
|---|---|---|
| G39 | 已补齐：K7按现行并发受理契约验证；Sept24多线程计划:154 | 自由worker/扰动/rerun各自验证收据、守恒与覆盖，不要求整局字节一致；立即恢复等价保留，真实恢复分支有非空收据。Account/Sealed按ADR双索引段验证，其余域连续性及全部失败负控保留。 JS 119项与Rust模块23项/恢复链1项定向短测通过，非作者独立重跑再审通过；未运行完整12tick默认fixture或矩阵。见 [实施复核](../implementation-gap-implementation/acceptance-tools-review.md)；原证据见 [工具](reaudit-tools.md)。 |
| G57 | 已补齐：诊断输出潜在大u64无损十进制字符串；diagnostics/量价CLI契约 | causal与DEV trace潜在大u64以无损十进制字符串跨JSON，Web严格同步tick与plan_changes，Inspector实际消费；不转Number、不把缺失Option造为零，不改正式交易金额契约。 大seed/数量/identity边界及实际trace parser/SSR短测通过，非作者跨层再审通过；独立于Q01 Money范围。见 [实施复核](../implementation-gap-implementation/tick-performance-review.md)；原证据见 [序列化](exhaustive-review/luna13.md)。 |
| G60 | 已补齐：性能工具通过现行启动选择进入游戏；性能README/正式命令 | 性能工具明确选择本地并提交现有启动操作，已启动时不重复启动，不修改产品启动政策。与G61共33项短测通过，非作者完整diff复核通过；没有跑真实浏览器性能矩阵。实现见 [验收工具](../implementation-gap-implementation/acceptance-tools.md)，原证据见 [旅程](exhaustive-review/luna48.md)。 |
| G61 | 已补齐：性能工具整体deadline及异常资源收尾闭合；测试/清理规则 | 正式性能入口用进程外300000ms supervisor；CDP失败显式拒绝，资源逐项收尾并聚合错误，sampler失败立即终止owned tree。与G60共33项短测通过、非作者复核通过；真实矩阵与跨平台收尾未实跑，不宣称其通过。见 [实施与复核](../implementation-gap-implementation/acceptance-tools-review.md)，原证据见 [UI工具](exhaustive-review/luna48.md) 与 [采样](exhaustive-review/luna64.md)。 |
| G62 | 已补齐：进程外deadline终止嵌套owned进程树并诚实报告；AGENTS/testing | 嵌套supervisor继承owned进程组，终止前枚举后代；Linux核对已观察PID退出，Windows等待并校验taskkill结果，未确认状态不冒称已终止，清理错误与原执行错误同时保留。22项相关短测通过，含真实阻塞child及detached孙进程，非作者再审通过；任意瞬时reparent及macOS/Windows实机仍未验证。见 [实施与复核](../implementation-gap-implementation/acceptance-tools-review.md)，原证据见 [树监督](exhaustive-review/sweep64.md) 与 [文案裁定](hidden-review/candidate-resolution-02.md)。 |
| G63 | 已补齐：普通Rust case独立10秒硬上限；AGENTS/testing | 普通与ignored清单取差集，每个普通case独立--exact进程及10000ms外部watchdog，按CPU预算并发并共享原批次期限；ignored长验收分类不变。9项新增定向短测独立重跑通过，非作者完整diff复核通过；另31项过滤环境敏感用例后通过，整文件曾超10秒，未宣称整文件或完整Rust回归通过。见 [实施与复核](../implementation-gap-implementation/acceptance-tools-review.md)，原证据见 [runner](exhaustive-review/sweep81.md) 与 [发布复核](exhaustive-review/luna77.md)。 |
| G72 | 已补齐：验收artifact先验证canonical containment再读内容；现存validator契约 | 新结果和PASS复用在读取bytes前验证canonical containment，保留源指纹与完整原始rerun校验；不扩为文件替换竞态或其他工具的全面安全声明。 先contain后read时序负控及JS定向短测通过，非作者完整diff复核通过；未运行完整验收。见 [实施复核](../implementation-gap-implementation/acceptance-tools-review.md)；原证据见 [原文复核](hidden-review/batch-080.md) 与 [裁定](hidden-review/candidate-resolution-02.md)。 |

## 3. 审计问题、用户回答与实施检查

用户于2026-10-05逐项回答下列审计问题；回答直接登记在对应条目，保留原审计证据，不把“已回答”冒充“代码已实现”。Q14已明确Simple与Simulation分离及完整功能目标，当前Simple具体期间、调用链与测试状态见Q14现位置；Simulation留待用户另分支，不据此核销。Q08已明确真实成交量加权均价及指标数据源边界，当前live Core七项、MA36项及Protocol双档fixture／Web45／root75 WASM／root78最终bundle限定状态见Q08现位置。其他已决项按各自 checklist 核对现有代码、补实现、短测和非作者复核后才能核销。Q01、Q05、Q06、Q07、Q12、Q17、Q18、Q20、Q21、Q24已实施或核实，Q11本人公告节奏与分类重估已接通，新增复核发现的 G43 淡出订阅回归已用真实红绿短测修复并独立复核；Q15仍缺原图视觉验收，Q22 queued/NPC stream 与三宿主 shared ingress、严格 generation 已实现并通过代表性验证，默认 cage 与独立同价 Browser 忙碌 step/NPC/cutoff 代表性 runtime 已通过并独立复核；新增多人 HTTP 忙碌受理及host31公共／本人事件和owner-scoped removed代表性短测绿色，相关非作者复核关闭有效发现。Protocol真实双档日终fixture、恢复深等及恢复续行NPC受理已通过，root75 WASM与root78最终bundle检查通过；跨Host E2E未执行，仅保留验收限制。Q10/Q19分别转G39/G80；此前完整回归的后续通过证据见[回归完成记录](../remaining-questions-and-features/regression-completion.md)，不外推为本批新提交已通过完整回归。编号为本审计内部ID，与docs/open-questions.md不是同一命名空间。每项均须核对生产代码、补短测并由非作者复核，不能用新增测试入口或文档代替实际接线。

| ID | 原审计事实与证据 | 用户回答与实施 checklist |
|---|---|---|
| Q01（已补齐） | 用户选择规范有符号 i64 十进制分字符串；Money、裸持仓成本、个人估值与因果诊断金额统一编码，Web 严格校验并以 BigInt 运算，实际生成类型同步。原疑问见 [基础复核](reaudit-foundations.md)，现行契约见 [ADR-0031](../../docs/decisions/0031-money-decimal-cents-wire.md)。 | 旧数字、非规范字符串与越界显式拒绝，无代际版本、兼容或迁移；AccountingAmount 元字符串与 u64 聚合范围不变。Money 29项及跨宿主代表性短测、Web类型检查与非作者复核通过；六个表示摘要由独立旧/新真实生产者逐原文金额 path 取证，不从失败输出重钉。曾在675ac4c复现的两项因果失败已随后补足真实成交fixture，原断言保留，14项短测通过，见 [因果修复](../remaining-questions-and-features/causal-test-fixture-fix.md)；未跑完整回归。见 [独立复核](../remaining-questions-and-features/money-wire-independent-review.md) 与 [表示证据](../remaining-questions-and-features/money-wire-golden.md)。 |
| Q02（访问及消费记账已接通） | 旧 record_public_history_read 没有生产调用的缺口已按[历史访问实施](../remaining-questions-and-features/q02-history-access.md)补齐；旧证据见 [策略与公司复核](reaudit-engine.md)。 | **用户回答：** NPC 本人决策实际读取历史才记本人经历；统一抽象层提供用户／账户访问，包括玩家，缓存操作及他人读取不算本人。<br>**实施 checklist：** ☑账户独立读取簿与个人价格锚点分离，NPC 历史实际参与决策后记账；☑玩家显式 K 线访问接通 EngineHost／Worker／Tauri／Server，严格响应及代际／换股隔离，缓存与 baseline 不记账；☑玩家记录显式请求在 Engine 成功产生响应的访问事实，不冒称亲眼看过或采纳，迟到响应丢弃不篡改已发生访问；☑日终保存／恢复、hash／shadow、严格当前存档及真实新局 fixture 接线，未引入兼容；☑Engine、Server 定向短测、Web 13项最终定向短测、类型编译与三组非作者完整 diff 复核通过。后续共享市场源码已将 Remote 历史请求按认证 subject 经 Server history_for 解析为本人账户，不再固定玩家0；身份6项、共享市场 REST5项实际绿色见[身份实施](../shared-market-accounts/server-identities.md)，但这些不是多人历史读取端到端专测，不能仅凭 auth／caller 存在核销该验收。全历史复制仍随返回量增长，不冒称分页或当前浏览器／WASM target 最终验收完成。 |
| Q03（已核实并同步正式文档） | 旧日历／会计文档承诺冻结 RegulationProfile，当前代码不存在该 Rust 类型。实现见 [基础复核](reaudit-foundations.md)。 | **用户回答：** 当前阶段只考虑不更新游戏时的表现；允许破坏性更新及旧存档失效重开，不需要跨更新冻结或兼容。<br>**实施 checklist：** ☑核实现行 simulation_policy_id 职责及严格校验；☑在 company-accounting 与 simulation-calendar 原有规则章节更正旧跨更新承诺；☑保留 CalendarPolicy 完整本局数据及 digest、当前结构严格恢复、个体参数和账务事实。非作者完整 diff 及源码复核通过；仅文档澄清，无新产品代码或回归执行。 |
| Q04（已核实并修正启动标题） | 旧 UX-CONTRACT 固定应用标题约定与详情动态股票标题不符，启动 HTML 原标题为 web。见 [界面复核](reaudit-ui.md)。 | **用户回答：** 手机详情顶部显示股票名称，返回首页后显示应用名称，保留该交互。<br>**实施 checklist：** ☑核实现有详情顶部股票名称；☑启动 HTML 与 UX 标题约定同步；☑真实 useMobileUiController 的 effect 短测覆盖首页→打开详情→切换股票→返回，7项定向测试通过，非作者完整 diff 复核通过；未跑浏览器视觉或完整回归。 |
| Q05（已补齐） | 用户已选择根test、现有手动开发CI和独立test:scripts共用完整scripts/**/*.test.mjs发现；源码指纹覆盖scripts目录。原疑问见 [工具复核](reaudit-tools.md)，实施见 [脚本测试入口](../remaining-questions-and-features/script-test-entry.md)。 | 普通case与每文件进程树10000ms，整批多核并行且共享300000ms期限，失败取消并等待清理；自身测试注入执行器不递归跑根回归。43项定向短测及非作者完整diff/独立复验通过；普通commit和产品发布仍不自动测试，未运行完整脚本集或完整回归。 |
| Q06（分配、设置与同局实际展示已补齐） | 初始持仓 spec 要求 ByKind 比例和约等于1、类内随机；旧实现只能表达一体化随机或类间权重加类内随机。原证据见 [基础复核](reaudit-foundations.md)，现行实现见[初始分配](../remaining-questions-and-features/q06-allocation.md)。 | **用户回答：** 以实际股份数量分配，百分比只是参考，允许整数舍入偏差。类间百分比／随机与类内百分比／随机均可选择；类间默认百分比。用户后续确认类内“默认等比例，可选为随机”，取代此前类内默认随机的回答；允许部分 NPC 初始化时没有股份。<br>**实施 checklist：** ☑`a1bce2c` 分离 BetweenKindDistribution／WithinKindDistribution，桌面与移动新局设置、启动／读档回填及严格当前存档接通，无旧形状兼容；☑整数股份守恒、默认 45／53／2 类间权重与类内 EqualPercentage、零持仓 NPC 合法，余股按 AccountId 升序分配；☑新玩家零股份，15项 Rust 短测、52项 Web 短测、实际类型生成／编译与真实日终 fixture 重建验证通过，非作者完整及增量复核通过；☑`8c420f2` 在同一个新建 Engine 开跑前只读实际初始股数／人数／零持股人数，三宿主查询与桌面／移动实际展示接通，不重复创建或消耗 RNG；fresh 来源资格只在内存保留、不进存档，tick0 读档也禁止把当前持仓冒充初始分配，读档清除旧结果、迟到响应隔离；☑实际展示3个 Engine 与2个宿主确切短测、30项 Web 短测、85项真实类型导出及 TypeScript 编译通过，三宿主 no-run 编译通过，非作者最终完整及增量复核通过；见[实际分配展示](../remaining-questions-and-features/q06-actual-allocation-preview.md)。未把编译或适配器短测称为真实浏览器 WASM runtime 验收。另以 `0ed5995` 修复小股本公司开账独立截断导致不平账，用实际融资约束资产用途，1项确切红测复现，修复后5项短测及非作者复核通过，未补投资者现金；见[小股本开局](../remaining-questions-and-features/small-company-opening.md)。未跑完整回归。 |
| Q07（日期与自然周期已接线，最新分钟消费者收口中） | 旧5／20交易日数量聚合曾按早期回答实现，最新用户明确取消，已由自然日／周／月／季／年与120／60／30／15／5／1分钟K取代；早期 `5c99c27` 日期前史及 `aeb979b` 真实档证据保留原时点，不恢复数量聚合兼容。 | **最新用户回答与实施：** ☑两端共用折叠周期选择器与MarketKlinePanel，保留精确rawPrices、自然周期真实量额／笔数、未知周期显式拒绝，实际caller与29项及共用消费者12项短绿／非作者复核见[K周期实施](../kline-periods/implementation.md)；分钟独立模块16项短绿及端点／午休／u64-u128／高位一分／量柱发现修复再审通过。☐分钟消费者与最新live契约的2个阶段case、当前生成契约及正式fixture仍在收口，不把选择菜单或旧日K聚合绿当分钟完整事实验证。日历日期属于行情表示，不改变各证券真实开闭市、T+1或撮合规则；未执行复杂回归仅记验证限制。 |
| Q08（真实均价、指标偏好、交割单与live事实链限定范围已接通） | 旧均价并非真实成交加权口径，见 [界面复核](reaudit-ui.md)；当前修复见[分时均价](../remaining-questions-and-features/q08-intraday-average.md)。 | **用户回答：** 玩家默认前端计算技术指标，可设置 Rust 数据；不支持时图案显示“不支持”，不静默 fallback。分时均价为当日 Σ（真实成交价格 × 股数）÷ Σ股数，图表不强制显示，Rust 按需计算；Rust 必须提供权威交易交割单，其他指标可选。MA周期可编辑、默认5/10/20/30/60、逐线开关，Desktop/Mobile共享本地偏好，不增加Rust必选指标。<br>**已接通及限定验证：** 移动端消费同帧权威日累计成交额／股数；Rust按需计算保留精确分子分母；前端/Rust偏好、unsupported显式状态、三宿主批量曲线及本人交割单分页／展示已接。历史Core12、Host1、Native guard1、Web schema6、五日UI6、私人日期Core6/Web7通过并独立复核。live Core当前7项通过且独立复核；Host Web5／Server route1限定短测及接线复核通过。MA生产接线完成，专项36项通过并独立复核，见[MA复核](../market-indicator-settings/review.md)。Protocol真实双档fixture经release Engine生成，恢复深等并续行三帧产生真实NPC受理；Web45合同短测、root75 WASM与root78最终Web bundle／Release WASM检查通过，见[公司系统交接](../company-system/current-handoff.md)与[实施清单](../company-system/implementation-checklist.md)。此前关于live新增2项、当前Protocol fixture未生成和正常WASM待收口均为旧状态，现已完成对应限定范围。三宿主非空交割单全链E2E、长期Browser、完整回归及Windows／macOS runtime未执行，仅为验证限制。未宣称全Host矩阵或Q08全部长期范围完成。 |
| Q09（已核实并同步正式文档） | 旧策略扩展及 GPU 优化路线的现行边界需澄清，见 [基础复核](reaudit-foundations.md) 与历史 [R04](coverage/r04.md)。 | **用户回答：** 策略内置，通过代码注册扩展，不做运行时插件；实时 GPU 继续不做，旧优化方案不强制照搬。<br>**实施 checklist：** ☑核实 StrategyFactory 内置分派与 CPU／Auto 权威计算、显式 GPU 请求报错；☑在 ADR-0006 原扩展段和 ADR-0008 原决策／边界段同步现状；☑D2／D3 已有主要生产路径，旧收益估算、触发线与 positions Vec 等仅历史候选，不新增实时 GPU 或运行时插件。非作者完整 diff 与源码复核通过；仅文档澄清，未新跑回归。 |
| Q10（已转G39） | 二次核对fixture→runner→生产step，确认未冻结实际受理轨迹。 | 不再作为未定产品方向；保留编号记录分类变化，运行结果仍待验收。 |
| Q11（本人信息节奏与分类重估已接通） | 旧生产 InstitutionDecisionRoot 仅分发 NewMaterial／HorizonExpired，更正年报走普通 λ 修订，现已接更正与真实合同逾期分类。原证据见 [策略与公司复核](reaudit-engine.md)，实施见[本人公告节奏](../remaining-questions-and-features/q11-notice-cadence.md)。 | **用户回答：** 用户所写 Q011 对应本表 Q11。机构及天天盯盘、活跃交易的散户关注相关信息，公告发布即可收到消息并更新估值；低频散户按本人每日一两次／每天／每周／每隔几天／每月等关注节奏，在后续本人关注时查看并重新评估。<br>**实施 checklist：** ☑`2bf5b2a` 保存逐账户 Immediate／Daily／EveryDays／Monthly 节奏，本人获知与交易执行分离，低频检查按本人时点，非交易日正式18:00公开后亦检查；☑`015a152` 修复新增复核发现的 G43 淡出订阅回归，去除 BeliefBook::entry_stocks 的无条件订阅兜底；当前订阅只来自本人持仓／关注／活动计划，历史信息与信念保留但不自动恢复订阅资格，真实日终经营公告的淡出负例及三类资格正例均经完整 save／restore 验证，真实红测0.20秒、绿测1.62秒及非作者复核通过，见[淡出订阅修复](../remaining-questions-and-features/q11-faded-subscriptions.md)；☑真实更正保留触发报告身份、直接重评，年度更正结合本人已知同 scope 较新中期重算，不混单体／合并范围；☑真实付款失败分 UncommittedExpense／ContractualPayable／ContractualOverdue／StatutoryPaymentFailure，仅实际合同逾期可触发 CreditDefault，不把信用恶化或无现金未执行支出冒充违约；☑成功日终登记本人 Acquisition，重估失败原子回滚，不写价格／历史阅读／成交经历，没有分析能力者保持 MethodDisabled，不强制下单或改 T+1；☑9个 Rust 确切短测、5个 Web／类型 parser 短测及85项实际类型导出通过，原批非作者完整及33文件暂存范围复核通过；原9项用例没有覆盖的 G43 边界现由上述真实红绿 case 补证，不把原批短测改写为当时已经覆盖。独立公开库 correct_and_publish 仍无可直接修改 Session 的宿主入口，不能冒称已经新增；此前共享真实 fixture 曾经生产者重生成并通过低层恢复／重保存及严格 parser；最新整批fixture仍在重生成，发现GameSession.save保留pending不等同Protocol公共EOD，已改用真实Protocol producer推进收口，当前尚不计新的公共EOD fixture完成，含新必填结构的集成产物尚未提交，不以此核销全部浏览器／WASM runtime、长验收或完整回归。 |
| Q12（已补齐） | 旧 CLI 将两个真实自由调度运行的 price_volume／causal 报告装入同一 JSON，无共同 run 身份。原证据见 [诊断](exhaustive-review/luna13.md)，实施见[同批运行](../remaining-questions-and-features/q12-same-run.md)。 | **用户回答：** 默认量价与因果诊断来自同一次运行，解释同一批成交；独立运行必须分别标识，不能混称一局。<br>**实施 checklist：** ☑`edd2f48` 以同一 GameSession 步进生成两报告，共同 run_id 与来源元数据关联真实 Trade receipt 和 causal 执行事实；☑成交笔数／股数／金额摘要必须对账，不一致显式失败；☑`--independent` 使用不同 run_id 并明确独立来源，seed 相同不冒称同轨迹；☑非空真实成交对账确切短测 6.16 秒通过，CLI 5项短测及非作者完整／增量复核通过。未跑长矩阵或完整回归。 |
| Q13（逐所日历与恢复已补齐） | 原 CivilClock 按首只股票决定共享日历，行情／计划／恢复误用共享交易日。原证据见 [日历](exhaustive-review/sweep04.md)，实施与红绿证据见 [逐所日历](../remaining-questions-and-features/q13-exchange-calendar.md)。 | **用户回答：** 自然日期共享，各交易所分别判断开市；休市交易所不接受交易、不生成假成交。<br>**实施 checklist：** ☑实际交易所集合决定共享市场会话，逐股开闭市独立查询，经营／披露继续自然日推进；☑休市股不受理新交易、不产生行情／分钟／日 K，不新添本人市场观察，仅本日开市股解锁T+1；☑计划、报价期限、估值锚点和价格样本使用本股开市时钟，账户经历与跨股接触驱逐保留共享时钟；☑严格恢复按实际逐所日期校验，ClosingAuction保留完成分钟240，复市有效计划不被共享day误判过期；☑异步入口仅成功发布日期可见，Protocol／Native用scope隔离的publication事务回滚，observer不泄漏失败候选，闭市StepFatal保留poison保护；☑host15实际engine49项及strategy2项短测全绿，Native13项与邻近plan25项实际短测通过，非作者完整Q13 diff复核通过。默认年度official仍为空、模拟回退明确标注；不新增schema／兼容／日内持久化，未跑完整回归。 |
| Q14（Simple营收／开支账面模型已定并实施中，Simulation另分支） | 已连续完整读取[668行最新蓝图](../remaining-questions-and-features/q14-financial-model-design.md)至EOF；旧资产缩放／三模板／净利率及SyntheticFunding提案不能覆盖最新回答。现有经营及客户基础历史证据保留，但不能证明当前Simple或Simulation已完整实现。 | **最新用户回答：** 最新Q14蓝图已按营收、固定开支和变动开支月度变化驱动利润，不再独立生成net margin／利润目标。Simple与Simulation共同完整财务、披露及股本功能，Simple仅账面展示、不追公司实际资金；不设SyntheticFunding周期／额度／资本注入，分红不因展示现金不足拒绝但仍检查可分配利润／方案，认购仍检查投资者本人真实现金、回购仍依真实委托成交。Simulation留给用户另一分支，本轮不实现，不据此核销其完整未来范围。<br>**本轮状态：** 父层CompanySystem／Simple、Session选择／严格恢复、统一信息披露、完整轻汇总财务与股本接线仍在工作树实施。host51 Core6项实际五红／一guard绿、Session4项真实红见[Core记录](../company-system/simple-core-contract.md)与[Session接线](../company-system/session-integration.md)；host55生产构建约50秒并不等于行为绿，月财务5项真实红后的候选绿尚在执行，未提前报告通过。必要短测、非作者复核及cohesive提交尚须按实际结果收口。<br>**历史有限模块：** `0b2cf88`客户债权人／联合typed来源29短绿及Bank真实source8短绿均只对应独立基础，非生产客户转账闭环；Simulation全部月度经营／有限现金／客户／研发／行业模块未来范围保留在蓝图另分支，不把本轮仅做Simple误写为已完成Simulation。Simple完整报告／税务／方案／投资者结算／严格日终存档仍必须实现，不用“简单”返回共同功能不支持；整个Q14未核销。 |
| Q15（文字对比度已修复，原图视觉验收待完成） | 旧图形颜色 token 的小字号白底对比与 AA 目标冲突。原证据见 [颜色](exhaustive-review/luna02.md)，实施见[颜色与对比度](../remaining-questions-and-features/q15-color-contrast.md)。 | **用户回答：** 保留红涨绿跌；文字颜色满足 AA，图形色与文字色分别定义；整体图样模拟、复刻同花顺手机版。<br>**实施 checklist：** ☑`2b85a34` 拆分文字／图形及实际明暗表面 token，保留 A 股红涨绿跌，不改变价格、成交量或柱形语义；☑短时 Chromium 按实际 CSS、透明祖先合成背景、computed style 及三个视口字号验证 AA，真实 MobileStockDetail／PositionsPanel／MarketGrid SSR 核对消费者；最终浏览器 case 3.31 秒、SSR case 0.50 秒通过，非作者四轮复核发现全部修复并再审通过；☑现有 HTML／SVG 仅作结构和风格草图，没有原图时不宣称像素级复刻；☐取得同花顺手机版原始参考并完成视觉对照，不把现有草图核销为像素验收。未跑完整 Web 测试、E2E 或完整回归。 |
| Q16（已核实当前 UI 口径） | 旧“负成本显示-而非xx%”混淆金额与收益率；非正净成本分析已修，见 [净成本修复](../remaining-questions-and-features/nonpositive-net-cost-fix.md)；原界面疑问见 [账户](exhaustive-review/luna15.md)。 | **用户回答：** 显示真实负净成本金额；以非正净成本作分母的收益率显示“-”，不显示误导百分比。<br>**实施 checklist：** ☑保留非正净成本合法分析；☑真实 PositionsPanel SSR 短测验证负成本 -10.00、浮盈 +4002元及持仓行无误导百分比，非作者复核通过；☑核对当前持仓 UI 没有成本收益率列，不为本项擅自新增列，也不声称已经显示“-”。将来若新增该比率，非正分母必须显式不可用并显示“-”；真实负成本和浮盈金额仍保留。未跑完整回归。 |
| Q17（已补齐） | 旧 ClosingEngine::correct 先过账／记录重述，再生成报告；合法派生汇总溢出可 Err 且留下部分状态。原证据见 [更正](exhaustive-review/luna23.md)，实施见[更正原子事务](../remaining-questions-and-features/q17-correction-atomic.md)。 | **用户回答：** 一次报表更正要么完整成功，要么保持操作前状态，不留下部分过账结果；失败原因与排查日志应保留。<br>**实施 checklist：** ☑`ffccc55` 对过账／重述／报告生成建立候选提交，PublicLibrary::correct_and_publish 追加公开形状、摘要及时序预检后整体提交；☑失败保持账套／结账登记簿／公开库原状，CorrectionFailed 保留理由和底层 cause，相同凭证失败未生效可重试；☑报表生成失败、加法溢出、后续期间不泄漏、更正守卫及公开更正失败／重试共5个确切短测通过，非作者完整及增量复核通过。没有新增不存在的 Session／UI 更正入口；CAS 28 原文取证债仍在，不把事务修复称为准则合规核验。未跑完整回归。 |
| Q18（已补齐） | 用户已选择最终Release collector也校验最低格式，与producer复用requireDistributionFormats；缺格式即拒绝。原疑问见 [制品](exhaustive-review/luna77.md)，实施见 [发行收集](../remaining-questions-and-features/release-collection.md)。 | 十组全部通过身份、文件/大小/摘要及格式校验后才创建收集目录，不改变unsigned或手动单产品范围。27项定向短测及非作者完整diff/独立短测通过；不是已有正常Release漏包的证据，未运行真实发版或完整回归。 |
| Q19（已转G80） | 新局默认Industrial不代表完整存档恢复拒绝Bank variant；恢复整体安装CompanyOperations，未校验EclPolicy。见 [银行续核](renewed-check/bank-restore.md)。 | 纠正原降级理由；缺口限SaveSlot外部恢复，不要求改变库级serde/issue_loan错误次序，也不称默认新局已具备完整银行产品。 |
| Q20（已补齐） | WASM句柄单调分配1..u32::MAX，0仅为耗尽哨兵；CAS保证并行不重号，HashMap Entry明确拒绝覆盖活会话。原问题见 [绑定](exhaustive-review/luna40.md)，实施见 [句柄耗尽](../remaining-questions-and-features/wasm-handle-exhaustion.md)。 | 耗尽返回ResourceLimit、不复用删除编号，已有会话保持有效；create/restore共同传播错误，无迁移或静默重试。14项短测及非作者独立重跑/完整diff复核通过；未冒称已创建数十亿会话或完成浏览器与完整回归。 |
| Q21（已核实并补真实受理测试） | 非正固定价可先报日限／price cage／资源拒绝，而非 OrderBook InvalidPrice；没有用户承诺的跨阶段错误优先序。原证据见 [校验](exhaustive-review/luna16.md)，现行核实见[交易校验与队列](../remaining-questions-and-features/q21-q22-current-audit.md)。 | **用户回答：** 多个非法条件没有特定报错优先级，只报一个原因也可以；允许检查与同类处理逻辑放在一起。NPC 可自身预检查或通过机制尽可能避免非正金额、余额不足等非法输入。<br>**实施 checklist：** ☑保留权威 AccountValidation／StockProcessing 校验与明确拒绝，既有拒绝收据、预留释放和空簿／零成交断言继续有效；☑测试与正式说明不要求未承诺的错误先后，仍证明被拒单零部分生效；☑`e791767` 在真实 T-1 NPC 决策入队后把现金降为零，再执行下一次 GameSession::step，验证 T 时 InsufficientCash、无受理／成交／NPC 挂单，确切短测 0.22 秒及非作者完整 diff 复核通过。NPC 预检不替代受理时最新资金／股份／价格校验，不把该代表性用例称为全部错误组合穷尽。未跑完整回归；Q22 的实际接收顺序缺口不随本项核销。 |
| Q22（queued、三宿主 ingress 与当前Protocol／WASM契约已接通，共享市场完整E2E保留验证限制） | 历史实现曾按来源／阶段排序并等待 NPC 整批 collect；`97e4953` 已改逐账户完成即 projection／登记 receipt。当前 Server/Tauri 直接共享入口、Browser 独立 shared-WASM intake 均接线，不再把这些旧反例当当前产品。见[队列核实](../remaining-questions-and-features/q21-q22-current-audit.md)、[共享宿主实施](../remaining-questions-and-features/q22-shared-host-ingress.md)；历史证据见 [草稿](exhaustive-review/luna49.md)。 | **用户回答：** 第一目标是提高多线程并行度、减少锁；来源排列不决定受理顺序，按先后受理。NPC 在 T-1 做完决策下单后异步进入委托队列，在 T 统一受理和撮合，以账户独立处理／减少共享锁提高并发。<br>**实施 checklist：** ☑per-session receiver登记真实账户／股票receipt并于T cutoff冻结；☑三宿主正式入口、canonical generation、旧capability关闭与内存入队确认；☑同账户Cash/Share及同股价格时间约束；☑Rust/WASM/Server/Web定向测试及非作者复核；☑默认cage代表性Browser runtime与独立同价witness已实际运行并复核；☑多人真实authenticated HTTP忙NPC受理、公共／本人金融事件投影、owner-scoped removed边界已短绿并复核。当前Protocol双档fixture由release Engine正规生成，真实恢复深等、恢复后继续三个frame并产生真实NPC受理；Web45契约短测、root75 WASM build与root78 final bundle／Release WASM check通过。关于正常WASM和Protocol fixture仍待收口的描述已过时，改按上述限定结果记录。长时间Browser、完整跨Host多人入队／撤单／保存／重开E2E、完整回归、长期运行及Windows／macOS runtime未执行，保留为验证限制；不因此声称整个Q22完整产品验收或绝对无锁。
| Q23（年度基准已实现，四行业／集团级联与发布排期收口中） | 旧公共 accrue_income_tax 重复调用会把已记税费计入再次税前并追加亏损池，现已修复；原证据见 [税务](exhaustive-review/luna54.md)，实施见[年度税务基准](../remaining-questions-and-features/q23-tax-idempotency.md)。生产经营 caller 继续按成功自然日日结在年末评估、每日按真实现金尝试缴税。 | **用户回答：** 同一公司、同一核算期间的同一次处理不能重复生效；新期间可继续计提，调整已有结果记差额；失败未生效允许重试。财报发布频率可配置月／季，默认季度；月报可直接选择预设方案，也可在设置中指定公开日期／时间，并可选择随机延迟，例如晚一两天。<br>**实施 checklist：** ☑`16dfbb2` 年度基准幂等、差额与失败原子边界及原批短测／独立复核已完成；☐当前源码已移除较早年度一律拒绝，并接年度级联、日终更正队列与固定集团候选，真实短测与审查发现逐项记录在[历史级联](../remaining-questions-and-features/q23-tax-cascade.md)和[集团更正](../remaining-questions-and-features/q23-group-tax-correction.md)，不沿用旧 HistoricalTaxReassessmentUnsupported 判断当前实现；☑四行业共用税 Owner、真实年度经营入口与税务／结账来源交叉恢复的限定生产小批已由 host22 8个 exact case 实际通过（0.01–1.94秒），非作者复核PASS，见[生产接线](../remaining-questions-and-features/q23-all-company-tax-production.md)及 `.tmp/checklist-wave4/host22-tax-*.log`；真实历史覆盖3项和地产组合 Owner6项见 `q23-build19-{0..2,4..9}.log`，地产时间回拨及一致性2项见 `host22-estate_*.log` 均实际绿色；这些不替代四行业全部结构化业务更正或完整税法验收；☑月／季设置、预设／自定义／可选随机排期及代表性真实公布链已有代码，host23月报6项 exact 短测全绿，总wall0.89秒；见[财报频率](../remaining-questions-and-features/q23-report-frequency.md)与 `.tmp/checklist-wave4/host23-*monthly*.log`。☑月报本人普通观察Monthly分类遗漏已按原报告ID真实红绿修复，root27绿；集团同范围封账／公开root24代表性case亦绿，限定复核通过，见[月报排期](../remaining-questions-and-features/q23-report-frequency.md)与[独立复核](../remaining-questions-and-features/q23-report-frequency-review.md)。☑`a6ba588a`固定本人已知同scope、同period内Annual＞HalfYear＞Quarter＞Monthly，种类优先于版本，4项短绿；☑`3f364bd`同窗同比按公开主附注真实window movement对上年同窗，root32四红／root33四绿并独立复核，见[同窗增长](../remaining-questions-and-features/q23-interim-window.md)；年度基准变化不重复学习中期增长的增强case亦绿并独立复核，见[年度基准复核](../remaining-questions-and-features/q23-monthly-baseline-review.md)。☐这些限定提交／短绿尚不表示完整月报契约已提交或当前WASM、最终Protocol EOD fixture已同步；完成当前生产契约、必要短测与非作者复核后可核销相应代码任务，不另以跨宿主E2E或完整回归作为必要门禁。发布频率与所得税核算、预缴／年度结算不是同一概念，不把季度发报理解为季度重新收全年税；结构化业务子账更正和完整税法边界仍须诚实登记。未核销整个 Q23，未跑完整回归。 |
| Q24（已补齐） | 初始化资源以成功取得为归属边界：第二个listener失败释放第一个；取得有效session ID后的baseline/capability失败等待本人stop_session。原问题见 [初始化裁定](hidden-review/candidate-resolution-04.md)，实施与短测见 [初始化回收](../remaining-questions-and-features/tauri-initialization.md)。 | 已取得资源独立并行清理并等待全部结果，AggregateError保留原错及每项清理错误；未知session ID不得猜测或误停别人。21项行为短测、TypeScript及非作者完整diff复核通过；独立另跑新增6例通过。不扩大为所有原生初始化故障或Worker回收已获完整验收。 |
| Q25（已核实并固定消费契约） | 旧身份候选见 [身份续核](renewed-check/belief-identity.md)，不存在必须强制双 profile 全等的缺陷。 | **用户回答：** 允许认知风格与执行风格分离。<br>**实施 checklist：** ☑ADR-0016 明确认知 BeliefBook 与执行 StrategyState 职责；☑建局默认同风格不构成恢复相等约束，合法分离可保存、恢复并被真实分析／计划消费；☑真实散户分析与机构 Growth 认知／DeepValue 执行 Patient 紧迫度两个短测通过，保留原确定性 strategy／attention、owner 与账户类别守卫；☑非作者完整 diff 复核通过。未新增无必要配置 API，未跑完整回归。 |
| D01（分红与股本行为已授权，待规则核验及实现） | 旧 [股东结算设计](../../docs/company-actions-design.md) 明确仅设计、不执行股东结算；此前边界需要按新用户决定修订，不能假称现有代码已完成。 | **用户回答：** 分红及其他股本行为按真实 A 股规则实施；其他股本行为不再逐项询问是否实施，直接核验官方规则。该决定修改旧“不执行”范围，但不允许无来源补钱或伪造成交。<br>**实施 checklist：** ☐依[分红规则研究](../remaining-questions-and-features/dividend-research.md)核验现行官方利润分配／股权登记／派息／除权除息及差别红利税；☐另核验送股／资本公积转增／配股／增发／回购的适用规则与股份、现金及税务处理；☐明确公司可分配利润和实际现金、方案决定及排期；☐处理全部有权股份、非交易股份持有人与税务身份；☐登记日权利固定、真实公司现金转账、税款及应付股利；☐除权除息参考价与行情口径；☐短测资金／股份／重复执行／日终保存及非作者复核。个人股息税B内核6项短绿仍不等于生产分红：恢复unknown disposed lot可抹去税款的P1新第7项仍待fresh红／修复／复核，见[税内核复核](../remaining-questions-and-features/cash-dividend-tax-review.md)，不把独立税模块当D01闭环。法规未规定的公司个体经济参数须明确为游戏配置，不凭旧设计默认实现。 |

## 4. 最小验证缺口与现有测试入口

下表保留静态审计阶段提出的代表性验证方向，不代表这些确认G仍待实现；当前已实施短测与独立复核结果以§2对应条目为准。G27的既有行为测试已在上一基线短测中通过，合并复核未重跑；未运行的真实浏览器旅程及长验收仍保留。普通case和整命令遵守10秒上限，不借审计启动全回归。

| 覆盖ID | 已有测试入口 | 还需验证的真实边界 |
|---|---|---|
| G01–G05 | `apps/server/tests/ws.rs`、`apps/web/src/host/remote-host.test.ts`、`apps/web/src/host/tauri-host.test.ts` | 浏览器可发凭据、speed授权、pull持续拉帧、推进后暂停继续不重置游标、失活恢复；模拟socket/人工Bearer测试不能代替浏览器契约。 |
| G06/G09 | `packages/engine/tests/fundamental_beliefs/gold.rs`、`packages/engine/tests/fundamental_beliefs/failures/guards.rs` | 独立逐期折现手算；本人已知中期材料更新且未获知材料不影响预测，保留年报/中期口径差异。 |
| G07/G08/Q02 | `packages/engine/tests/analysis_profiles/invariants.rs`、`packages/engine/tests/experience_feedback/reads.rs`、`packages/engine/tests/technical_memory/memory.rs` | 真实GameSession散户消费档案与日期衰减；实际历史读取留痕；不能仅重测纯函数。 |
| G10–G14 | `apps/web/src/app/market-chart-runtime.test.ts`、`apps/web/src/mobile/market-model.test.ts`、`apps/web/src/mobile/mobile-component-render.test.ts` | 生产hook同分钟多帧/跨日/竞价转连续，最新七笔不同tick，涨跌颜色；已有日K增量用例不代表分时链已覆盖。 |
| G15 | `packages/engine/tests/calendar/exchange_days.rs` | 官方覆盖既能增加也能取消模拟休市；周末规则、无官方覆盖回退分别保留。无需联网行情。 |
| G16–G19 | `packages/engine/src/indicators.rs` 的测试、`apps/desktop/src-tauri/src/actor.rs` 的测试、生产性能入口 | 固定活跃工作量增加终止计划，检查复制工作；并行/发布/有界背压或等价机制进入真实生产链，性能结论另需测量。 |
| G20/G21 | 新局启动路径、`packages/engine/examples/price_volume_baseline.rs` 的seed测试 | 普通新局取种与测试注入分开；setup合法但无关存档字段异常时符合CLI契约。 |
| G22–G25 | 移动组件测试、`apps/web/e2e/mobile-layout.spec.ts` | 字段关联、进入/返回焦点、滚动保持及实际点击热区；未执行本轮视觉矩阵。 |
| G26 | `scripts/ci-workflow.test.mjs` | 手动开发CI中的warning退出状态；不把lint加入发布链路。 |
| G27（已核销） | `scripts/publish-release.test.mjs` | 上一基线复核通过正常发布二次SHA查询及上传期间标签变化保留draft的既有测试；本轮未重跑，不等同于线上发布验收或原子标签锁。 |
| G28/G29 | `packages/engine/tests/consolidation/`、`packages/engine/tests/industry_reports/`、`packages/engine/tests/session.rs` | 固定集团报告由真实日终生成并公开、无集团明确不适用；零NPC/正流通盘/ByKind与Random对照，仍拒绝非法权重。 |
| G30–G34 | 移动组件及chart runtime测试、`apps/web/e2e/mobile-layout.spec.ts` | 可见焦点、未变股票保持图表引用、中轴坐标一致、容器字号及reduce偏好；不能仅SSR或源码字符串断言。 |
| G35–G38 | `packages/engine/tests/industrial_accounting/`、`packages/engine/tests/company_operations/`、`packages/engine/tests/diagnostic_parity.rs`、`packages/engine/tests/plan_allocation/` | 代表性月结折旧/所得税/商业债务支付、四行业自定义会话日结查询、DEV真实订单关联、新旧买计划有限现金竞争；纯处理器测试不替代生产入口。 |
| G39 | `scripts/simulation/escrow-verification-contracts.test.mjs`、`scripts/simulation/run-escrow-verification-matrix.test.mjs` | 区分固定受理事实重放与自由调度，合法局部顺序差异不误判，同时仍拒绝资金/股份/价时/依赖错误。无需本轮运行完整K7。 |
| G69–G71/G73 | `packages/engine/tests/plans.rs`、`packages/engine/tests/industrial_accounting/`、`packages/engine/tests/company_operations/failures/scheduler.rs`、`packages/engine/tests/save_contract/` | 非零/可表示期限含MAX/1，库存科目无seed对账，重复调度身份及耗尽错误，保险损坏子账serde拒绝；分别核直接API与完整恢复，不伪造合法经营或正常局故障。 |
| G77 | `packages/engine/tests/consolidation/` | 配对相等的零/负额仍须显式拒绝，合法正额和已有配对/账面上界负控保留；不以禁止编辑合法资产代替校验。 |
| G78/G79 | `packages/engine/tests/company_operations/`、`packages/engine/tests/save_contract/`、工商账套短fixture | 时钟重复ID、耗尽及未来pending日期超政策边界显式拒绝，合法未耗尽/范围内日期继续注册；授信计算失败与真正无授信分开，保留合法编辑资产事实。 |
| G80 | `packages/engine/src/company/bank/behavior_tests.rs`、`packages/engine/tests/save_contract/` | 完整Bank变体存档对空表/非法权重拒绝，合法政策及可编辑资产保留；独立serde接受集合测试不等于完整SaveSlot已通过。 |
| G72/G74–G76 | matrix短fixture、`pipeline/adaptive_plan_chain_tests.rs`、`packages/engine/tests/calendar/` | matrix canonical判断先于内容读取，Continuous与Auction两个AccountValidation结果身份预检入口失败后停用，CivilInstant秒域与政策source摘要变化的身份一致性；保留合法路径，不运行完整matrix或完整回归。 |

新增G40–G68的短验证应对应真实消费链：异步控制确认/取消/重同步终态、非推进generation、现金不足后日结/恢复/公开查询、超过8只非保护记忆及淡出后重新发现、零/非零量和间断null、1/2/5档盘口、半分正负成本、中文/键盘/选中状态、React/IPC错误、无行情baseline、非默认setup、库级非法输入、多lender和承保期限。工具项用阻塞/采样失败短fixture验证整个子树在原期限内退出，并保留守恒和真实受理重放负控；不能靠删断言、放宽deadline或恢复自动发布测试解决。各项实施记录给出对应入口与实际短测结果，见§2；这些结果不替代未运行的完整回归、长矩阵或真实浏览器旅程。

## 5. 已实现与旧要求核销

- **G27已核销：** `scripts/publish-release.mjs:109` 在draft上传和远端资产核验后重新查询tag SHA，变化时抛错并保留draft，113行才执行公开。`publish-release.test.mjs` 验证正常二次查询顺序及上传期间移动标签时禁止公开；上一基线复核定向测试通过，本轮代码未变且未重跑。它兑现了原缺失守卫，不宣称GitHub提供了原子不可变标签锁。详见 [工具复核](reaudit-tools.md)。
- 公司公开报告刷新选择已修正：`CompanyPanel.tsx:60` 仅在ready/empty协调选择，loading/error不再因临时空列表抹掉用户选择。新增组件测试覆盖临时状态、真实空结果和换公司回退；本轮仅核对源码，没有重跑浏览器或这些组件用例。此为已修行为，不新增待办编号。

- 更新后的 [核心账户/撮合](reaudit-core-contracts.md)、[Session/pipeline](reaudit-pipeline-contracts.md)、[账套/报告](reaudit-accounting-contracts.md) 复核未确认相应既有契约在重构中丢失；这不是完整回归通过声明。
- 当前新增测试按实际成交事件和收据核对现金加实收费用、股份、日K量额笔数及日界；立即恢复仍要求存档字节一致，不再要求两个自由调度实例的未来成交完全一致。规模测试另核对计划引用、原有计划保留、历史公开材料不变和公开时间边界；DEV查询测试改为同一会话查询前后状态不变，Server测试显式区分诊断feature。这段记录的是静态审计阶段已读的测试契约改进，当时不能据此核销G37/G39；后续真实生产接线及短测已在§2核销相应确认缺口，不核销长期规模验收债。
- 底层撮合中途溢出的部分写入、房地产计息 post 后子账更新失败在旧基线已有。新测试固定旧失败顺序不等于本次引入故障；Session 候选回滚与底层方法边界须区分。没有证据证明历史要求承诺这些底层方法全部强原子，因此不新增 G。静态追踪未确认正常默认局或存档恢复链存在该复现路径，未运行相关场景。

- A01–A11的主要生产能力已经存在：公开财报、远程查询方法、个体机构风险、冻结UrgencyPolicy、日终最小存档、内存日结回滚、工作台拖拽、错误详情、Rust指标、账户/订单增量及DEV当前宿主诊断。G项是局部断链/边界，不能用局部已实现证明整个宿主或整个策略模块无缺陷。
- 共同隐藏V/TrackV、资金循环/补钱、公共日内存档、任意挂单配额、旧格式兼容、按来源固定交易优先、自由并发整局字节一致已经被替代或明确排除。
- `AllocationExperience::default()` 不单独列缺口：机构信心已在上游按真实失败/净获利退出调整，成本与风险也走个人阈值；再接旧失败helper会重复扣信心，不能恢复统一20日强卖。
- 360根负时间虚拟日K、真实成交更新量额、T+1/费用/占用、开收盘撮合、符号最高/最低限价、初始持仓、账户结算、计划执行与日终子单清理均有生产实现。
- 8MiB远程存档上限、午休时钟遗漏、公共财报期间格式、WASM空值、旧测试使用玩家快照查NPC等历史发现已经有后续修正；不沿用旧REJECT或保留二进制失败判断当前源码。
- 七个手动入口、三平台打包、纯Server/WebUI Server、Release、Pages与缓存清理已有代码及后续发布记录，见 `docs/build-and-deployment.md:300`。本轮没有重新请求GitHub或重新验证线上状态。
- 普通commit/PR不自动CI、macOS/Windows不签名，以及发布不调用CI/测试/lint/smoke，均为用户决定，不是待恢复的缺口。
- 旧 sealed corpus 适配器、重放 example、bundle 装配与旧测试逐字冻结工具已经退役；`docs/test-cleanup-checklist.md` §12 明确接受历史证据不再可执行复验。不得把旧 zero-cash witness、映射表缺席重新登记成现行代码任务；保留 helper 仅测试调用是获批范围，不是生产接线遗漏。详见 [删除历史核销](coverage/h01.md)。Q05 与 G39 各有独立现行契约，不随旧工具退役一并核销。

## 6. 尚待完成的未来能力与保留边界

用户已要求完成未定 Q 项及其余未来能力；[旧盘点B表](../../docs/implementation-gaps.md#3-确实没有完整实现但需确认范围或属于未来扩展) 不再因“未来”标签而免于实施。仍保留用户明确边界：不做日内持久化／WAL、不补已有投资者现金、不接真实行情或真实市场统计、不做实时 GPU、不做平台签名／公证；不增加存档版本兼容、迁移或默认补字段。旧架构提案中的具体优化路线不强制照搬，能力是否完成仍需生产接线与相应验收证据，不能以历史提案或占位 UI 核销。

| 类别 | 未完成能力 / 当前边界 |
|---|---|
| 行情与内容 | 永久历史Core12项、Host请求1项、Native恶意存档1项、Web严格schema6项、五日UI6项及私人日期Core6／Web7项已短绿并独立复核，见[Core复核](../retained-history/core-independent-review.md)及[日期交割单](../retained-history/trade-date-pagination.md)。实时分钟Core当前7项通过并独立复核，Host Web5／Server route1限定短测与接线复核通过，见[live Core](../retained-history/live-core-review.md)与[live Host复核](../retained-history/live-hosts-independent-review.md)；不再保留“新增2项待fresh”的旧状态。Protocol双档fresh fixture已由release Engine生成，真实restore深等并续行三帧产生NPC受理；Web45、root75 WASM构建、root78最终Web bundle／Release WASM check通过，见[公司系统交接](../company-system/current-handoff.md)与[实施清单](../company-system/implementation-checklist.md)。当前自然日／周／月／季／年及120／60／30／15／5／1分钟K折叠菜单caller已接线，旧5／20数量聚合取消，见[K周期](../kline-periods/implementation.md)。MA默认5／10／20／30／60、可编辑周期、逐线开关与单Redux共享消费者已接线，专项36项短测及非作者复核通过，见[MA复核](../market-indicator-settings/review.md)；完整回归、长期Browser、全Host E2E及多OS runtime未执行，不将其误写为代码缺失或已通过。看点／资讯／社区／简况、首页资金／资讯／资产／分析快捷页、更多分类与独立收盘竞价指示曲线仍未核销，收盘撮合已有。 |
| 玩家产品 | 多存档槽管理已接统一 ArchiveStore、浏览器 IndexedDB 与 Native SQLite；浏览器真实事务／管理UI及选择失败边界短测见[浏览器存档](../day-end-storage/browser.md)，Native真实槽管理／物理重启证据见[SQLite实施](../day-end-storage/native-sqlite.md)。这不核销成就、完整个人交易流水／复盘或云同步；100条成交带不是完整流水，云同步仍须保持日终持久化边界，不保存日内挂单。 |
| 部署与运维 | 用户选择的用户名＋密码、匿名登录、公开主体加入同一市场及成功日终数据库存档已接线，不再整体称为尚无实现。浏览器WASM用访问者IndexedDB，Server／Desktop本地用SQLite，Remote不运行WASM而以Server为持久化源，见[ADR-0033](../../docs/decisions/0033-day-end-database-and-browser-storage.md)。Native实施及身份接线的限定短测与最终复核见[Native实施](../day-end-storage/native-sqlite.md)、[身份实施](../shared-market-accounts/server-identities.md)及[Server最终复核](../shared-market-accounts/server-final-review.md)。Protocol双档真实fixture、restore深等／NPC续行、root75 WASM及root78最终bundle／Release WASM check已通过，详见[公司系统交接](../company-system/current-handoff.md)与[实施清单](../company-system/implementation-checklist.md)；不再把这些现行fixture／构建项目标为待同步。Windows／macOS writer-lock及跨Host E2E仍未实机运行，作为验证限制，不冒称通过。TLS／Origin／运营控制、自动更新功能仍待推进；跨客户端完整验收未执行。存档兼容、迁移、WAL及平台签名／公证禁止。 |
| 计算与长期架构 | 冷热历史／区间查询、长期性能与恢复边界、2099年后日历支持等能力仍需实施或补验收；蒙特卡洛、页级 COW、反向唤醒索引及旧观察令牌方案需按现行能力目标核对，不机械复刻 ADR-0018 的旧路径。实时 GPU 内核与日内 WAL／durable 水位明确不做，G16 已完成的局部所有权目标不能核销全部长期能力。 |
| 领域扩展 | 清算、额外市场板块／订单类型／停复牌／融资融券、高级银行保险／集团会计模式、复杂学习／社会传播／组合风险、第二语言等旧 B 表能力仍须逐项核对现行范围、实现与验证，不用“未来”整体排除。分红及其他股本行为按 D01 核验官方规则后实施；交易规则与会计文档现有简化描述只是当前边界，不是未来功能已完成的证明，也不能把股本行为授权当作清算法则依据。 |

### 仅缺数据依据或验收证据

官方休市原文覆盖、部分会计/税务依据仍有取证债；不使用真实行情不等于可以编造制度。
其中 CAS 8 减值原文在政策 fixture 中仍标 blocked，但工商日结已有减值调用；须补法源或显式登记游戏假设，不能称“尚无减值代码”或“已经核验准则合规”。详见 S03-C1 与候选核销记录。
稳定多线程收益、历史年龄矩阵、三宿主跨日真实旅程、安装器GUI/运行库兼容、移动/Wayland视觉及完整统计不能以短测或源码存在核销。[main最新验收](../main-release-validation/summary.md) 登记了绑定 `2247f4f` 产品树的默认/all-feature回归、脚本、12个Chromium E2E、九项ignored及build-only test Release/Pages的结果，本轮逐章核对来源和代码，没有重跑或重新查询GitHub。10万完整日档591,344,527 bytes超过Server有界解码536,870,912 bytes；typed恢复通过不证明Server能加载，WASM/Desktop入口又不同，不能泛称三宿主统一限制。K7 after/sensitivity fresh矩阵、真实UI性能和安装器GUI仍无本轮补证；G39/G60–G63明确列代码契约缺口。
旧host-parity/release-contract/verify-plan名称未找到，现有WASM导出、制品manifest、K7验证各有不同覆盖范围；缺的是未被替代的真实验收能力，不要求按旧名重复造工具。

### 文档漂移另行登记

旧 ADR-0008 曾称 Rust 指标／跨股撮合“待落地”，Q09 已澄清主要能力与旧优化路线；量价正式命令示例现已包含 simulation-diagnostics feature，因果诊断文档已把漏午休记录限定为历史复核，不再称当前缺口。旧财报披露模块封账不可达注释等其余文案仍需逐项核对，不能将这些局部澄清外推为全部历史文案已修复。
报告批准08:00的假设登记、年报日期fixture算术文字也需校正，不能混成新的会计产品。
旧Money设计的负数舍入示例、GameConfig佣金最低额示例有算术错误；ADR0020状态、ADR0008旧规模耗时及positions Vec路线需与当前实现区分。
量价清单的散户异常选股仍是未来扩展，机构曝光权重不能证明散户已接线；tick-only诊断不能证明完整公司自然日经营/披露验收。
历史中文化、handoff和验证签署还存在证据闭环债：group08复核状态冲突、domain摘要指向另一版本、views/delta指纹已变化、strategy specificity未通过项未解释处置，以及测试文书两项建议缺完成证据。逐项见 [材料完整性记录](hidden-review/README.md#材料完整性与可追溯性)。本次不改写这些原历史证据，也不把缺证当作产品G；只有本轮报告失实的当前状态/ADR措辞被纠正。
补充全文中发现的旧 API/算术/文案差异也不得机械转成新功能：账户总资产已在会话个人权益计算中消费，不要求恢复同名 `Account::total_assets`；Money 小数解析在 ASCII 和长度校验后使用 `expect`，只是与旧计划的禁用写法不一致，未证明存在可触发的解析 panic；初始 HTML 标题 `web` 在 App 首次 effect 后会更新，启动壳与运行时标题范围需区别。逐项主控复核见 [候选核销记录](candidate-checks.md)。

## 7. 需求覆盖对照

本节保留初次全文审计建立的契约族映射及原基线代码位置；本轮已按 [当前全文记录](coverage-index.md) 重核其具体承诺。局部反例及后续取代以第2、3节为准，当前行号见新记录，不用旧行号定位新版本。

本节由各批补充的章节/任务对照收口；“已实现”仅表示该契约族存在生产路径，未承诺通过当前基线运行验收。局部反例以G/Q表为准。

表中 R01–R20 对应本轮 20 个并发文档组；S01–S06 对应上轮其余全文批次。
这是原概述编号；最新逐篇证据采用 `coverage/r01.md`–`r20.md`、`s01.md`–`s31.md`、`h01.md`，按 [索引](coverage-index.md) 查阅，不把两种 S 编号混用。
“已有”指生产契约族，不表示整篇每项完成；各组的局部缺口必须同时读取第二、三节。
路径未附行号的测试目录只作检索入口，不是执行结果。历史计划的 RED/GREEN、提交与最终 DoD 统一归验收，不重复当作运行功能。

| 组 | 原文覆盖章节/任务族 | 生产路径与测试源码入口 | 判定与后续 |
|---|---|---|---|
| R01 | ADR-0018 §1–15：版本根、历史、观察、提交、COW、WAL、长期性能；ADR-0025 全部保存/加载契约 | `packages/engine/src/session/protocol/civil/session.rs`、`apps/web/src/save/day-end-persistence.ts`；`apps/web/src/save/day-end-archive.test.ts`、`packages/engine/tests/save_contract/` | 公共日终候选、加载隔离已有；内部 quiet-point 快照不违反日终档。长期历史／查询能力按第6节继续推进，具体版本根／COW 路线不强制照搬；日内 WAL 明确不做，已完成的局部复制目标见 G16。 |
| R02 | ADR-0017 P0–P9、双账本、预算、股票任务、回滚、计划续行及全部验收；escrow计划各波次 | `packages/engine/src/session/pipeline/authoritative_tick.rs`、`continuous_tick_transaction.rs`（同目录）、`candidate_commit.rs`（同目录）；该目录 `adaptive_plan_chain_tests.rs`、`pipeline_contract.rs` | 实际权威入口接线；来源类固定优先和跨线程整局字节一致被替代。长期吞吐/全门禁属验收，不能用历史REJECT断言现在失败。 |
| R03 | ADR-0016 全文；会计§1–7、四行业/报表/日历/法源；公司行为§1–4 | `packages/engine/src/session.rs` 日终经营/结账/披露；`packages/engine/src/accounting/closing/mod.rs`；`packages/engine/tests/industry_reports/`、`packages/engine/tests/consolidation/` | 默认工商日常经营/单体披露已有；固定集团G28、工商期末G35、四行业会话G36；Q03规则冻结按用户回答核实。分红及其他股本行为按 D01 核验并实施，清算不因股本行为授权而自动扩展；法源债不冒充代码缺口。 |
| R04 | ADR-0006 策略边界/独立参数/工厂/注意力全部修订；ADR-0021 仓位/报价/费用；ADR-0026 用户决定及补漏 | `packages/engine/src/strategy/factory.rs`、`packages/engine/src/session/institutional_behavior.rs`、`packages/engine/src/session/decision_chain/quote.rs`；`packages/engine/tests/experience_feedback/`、`packages/engine/tests/urgency/` | 个体风险/成本/恢复、合法报价已有；G06–G09/G38为不同环节。主动成交可用Highest/Lowest限价，不要求PlaceMarket。sealed注册扩展方式是架构澄清，非当前策略失效。 |
| R05 | ADR-0011 分钟/日窗口/等权市场/个人风险；ADR-0012 #1–10；ADR-0013 原始记忆及K5修订 | `packages/engine/src/observation.rs`、`packages/engine/src/session/pipeline/decision_snapshot_capture.rs`、`retail_projection.rs`（同目录）；`packages/engine/tests/observations.rs`、`packages/engine/tests/experience.rs` | 观察、真实成交记忆、原有行为已有；日期与20日衰减漏接见G08。内部save不等于公共日内存档，不能重开已核销项。 |
| R06 | ADR-0009 全阶段/坐标/事件/严格档；ADR-0014 尾盘撮合/前端边界；ADR-0015 母单/生命周期/K6 | `packages/engine/src/session/pipeline/stock_auction.rs`、`packages/engine/src/session/plan_execution.rs`；`packages/engine/tests/auction.rs`、`apps/web/src/mobile/market-model.test.ts` | 开收盘撮合、母单协调和日终清理已有；尾盘独立曲线明确未来；分时UI局部错误见G10–G14。 |
| R07 | account计划Tasks1–7、spec§1–10：账户/策略骨架/成本/T+1/结算/快照 | `packages/engine/src/account.rs`、`packages/engine/src/session/pipeline/settlement.rs`；`packages/engine/tests/account.rs`、`packages/engine/tests/session.rs` | 已有；生产用receipt→apply_settlement而非旧apply_trade，不按旧入口漏调误报；账户整体serde被显式档映射替代。 |
| R08 | orderbook计划Tasks1–7、spec§1–9：类型/构造/校验/价时撮合/撤单/深度/序列化 | `packages/engine/src/orderbook.rs`、`packages/engine/src/session/pipeline/continuous_matching.rs`；`packages/engine/tests/orderbook.rs` | 已有；ID由Session分配、深度用u64、零价拒绝是演进；spec中的命名字段错误与实现元组变体为文档漂移。 |
| R09 | market计划Tasks1–5、spec§1–10：构造/涨跌停/撮合/旧V/日终/盘口 | `packages/engine/src/market.rs`、`packages/engine/src/session/pipeline/auction_day_end.rs`；`packages/engine/tests/market/` | 成交价、限制、日终已有；V演化已由ADR0016及公司计划明确删除，不登记为未来必须恢复。整数基点涨跌停优先于旧Money.apply_rate草案。 |
| R10 | session计划Tasks1–7、spec§1–9：装配/玩家队列/推进/投影/存档/错误 | `packages/engine/src/session/failure.rs`、`packages/engine/src/session/snapshot.rs`、`packages/engine/src/session/protocol/civil/session.rs`；`packages/engine/tests/session.rs` | 已有；原子TickShadowPlan候选取代旧失败后继续，公开玩家投影不泄露全部NPC状态，集合竞价不再是未来。 |
| R11 | strategy-impl计划Tasks1–6、spec§1–11：三策略/多股视图/RNG/工厂/验证/分层 | `packages/engine/src/strategy/mod.rs`、`packages/engine/src/strategy/factory.rs`、`packages/engine/src/session/pipeline/npc_decisions.rs`；`packages/engine/tests/strategy.rs`、`packages/engine/tests/strategy_state.rs` | 主干已有；Value/TrackV被个人信念替代，Momentum改用完成分钟。不能据旧骨架完成核销G07/G08。 |
| R12 | initial-positions计划/spec全部任务；gameconfig全部配置/拒绝/默认/费用契约 | `packages/engine/src/session.rs` 新局分配、`packages/engine/src/config.rs`；`packages/engine/tests/session.rs`、`packages/engine/tests/config.rs` | 初始持仓和费用主干已有；零NPC边界G29，分配政策Q06。旧隐藏V/统一仓位上限不恢复；金额范围Q01，新局取种G20。 |
| R13 | money计划Tasks1–7、spec§1–9：分/溢出/解析/费率/半偶/serde/导出 | `packages/engine/src/money.rs`、`packages/engine/src/config.rs`；`packages/engine/tests/money.rs` | 已有，库级字符串解析不要求生产必须调用；跨端范围见Q01。spec把精确-5写成-4是旧算术错误，不是应恢复的预期。 |
| R14 | 两份公司计划K1–K7、任务1–42、F1–F4及完成条件 | 日历/会计/披露/个人信念/计划→`packages/engine/src/session.rs`；`packages/engine/tests/company_decision_session/`、`packages/engine/tests/company_scenarios/`、`packages/engine/tests/company_scale.rs` | 主要模块与宿主入口已有；G06–G09、G15/G16、G28/G35–G38、Q02/Q03等不能被任务勾选掩盖。W6长验收/统计/视觉证据单列，不宣称全绿。 |
| R15 | Sept24单局多线程；Sept25生产入口/线程池；Sept26 ready receipt/线程边界全部步骤 | `packages/engine/src/session/pipeline/ready_ingress.rs`、`packages/engine/src/session/pipeline/stock_stream.rs`；`packages/engine/examples/production_entry_performance.rs`、流水线相关测试 | 生产并行受理/股票工作主干已有；历史plans复制见G16；稳定多核收益属于需实测的验收，不从Rayon存在推导。 |
| R16 | sparse-continuous-book-feedback全部任务；量价清单已定/待定项及CLI；ADR-0022符号限价全文 | `packages/engine/src/session/pipeline/continuous_tick_transaction.rs`、`packages/engine/examples/price_volume_baseline.rs`；`packages/engine/tests/orderbook.rs`、pipeline测试 | 符号报价/实际受理解析已有；CLI输入投影G21。统计必须用虚拟前史＋真实游戏撮合，不恢复真实行情授权等待。 |
| R17 | DESIGN全部页面/部件/尺寸；UX全部显示与Flow ledger；移动QA全部清单 | `apps/web/src/App.tsx`、`apps/web/src/mobile/MobileStockDetail.tsx`、`apps/web/src/app/useMarketChartRuntime.ts`；`apps/web/src/mobile/mobile-component-render.test.ts`、`apps/web/e2e/mobile-layout.spec.ts` | 已有页面不能核销G10–G14/G22–G25/G30–G34；Q04标题、Q07/Q08派生口径冲突；内容占位/更多周期未来，视觉与平台矩阵尚需验收。 |
| R18 | ADR-0008 D1–D5/N1–N3/T1–T6及后续；ADR-0020全部；tech-stack全部选型/门禁 | `packages/engine/src/compute.rs`、`packages/engine/src/indicators.rs`、`packages/engine/src/lib.rs`；`packages/engine/tests/compute.rs`、`scripts/ci-workflow.test.mjs` | Rayon 权威路径、Rust 指标、平台 allocator 已有；ComputeBackend 是库级接缝非会话切换，G17／G26 已登记。实时 GPU 明确不做；Q09 已澄清旧 positions Vec 等优化路线不强制照搬，Q08 数据源选择仍须实际接线。 |
| R19 | ADR-0005三宿主/取种/协议/调度/心跳；ADR-0010更新/节奏/背压/订阅；ADR-0007前端框架/交互 | `apps/web/src/host/`、`apps/server/src/actor.rs`、`apps/desktop/src-tauri/src/actor.rs`；宿主适配器测试及 `apps/server/tests/ws.rs` | 三宿主骨架/局部刷新已有；真实端到端缺口G01–G05/G18–G20；框架选型不等于重连或背压完成。 |
| R20 | 两份resolve-blockers-wayland计划全部任务/验收；公司archive索引与适用边界 | `scripts/performance/`、`scripts/desktop/`、宿主与协议生产入口；相关脚本测试和历史证据 | 历史修正不能回退成现行缺口；Wayland/GUI/K7最终证据债保留，归档不新增产品要求。 |
| S01 | trading-rules、simulation-calendar、ADR0019/0023/0024现行范围全部 | `packages/engine/src/calendar/holidays.rs`、`packages/engine/src/session/candles.rs`、市场/结算；`packages/engine/tests/calendar/` | 合成前史/撮合/不补钱/交易简化已有；G15覆盖替代边界，Q03政策关系；不得把未支持市场制度写成已实现。 |
| S02 | 根工程守则/README、architecture、principles、error-handling、naming、ADR0000–0004、Git与贡献说明 | Rust engine依赖边界、RTK投影、宿主启动/错误入口；`apps/web/src/App.tsx`、workspace manifests | 架构主干已有；G22字段错误；模板/协作规范不算新增产品功能，历史命令/路线差异按新决定核销。 |
| S03 | ADR0027/0028、build-and-deployment、actions-cache、ci-build-fixes全部目标/权限/运行边界 | `.github/workflows/`、`scripts/build-targets.mjs`、`scripts/publish-release.mjs`、`scripts/prune-actions-cache.mjs` | 七按钮、三平台制品、Pages、标签发行和清理已有；原G27守卫窗口已由当前代码补上并在第5节核销。普通提交无自动任务/不签名是决定；线上状态未在本轮重验。 |
| S04 | testing、test-cleanup、diagnostics/causal、naming-refactor、performance说明全部 | `scripts/run-web-tests.mjs`、`scripts/performance/`、引擎diagnostics与性能examples | 工具存在不等于完整验收；Q05发现策略；旧脚本被替代、旧午休诊断文字过时；长期/统计/真实宿主矩阵单列。 |
| S05 | implementation-gaps、roadmap、work-status、open-questions全部 | A01–A11对应生产代码；第二节反例与第六节未来范围 | 原批完成记录保留，但不外推整个模块无缺口；Q 项以最新用户回答与实际实施分开登记，B 表除明确保留边界外按第6节继续实现，不再以未来标签排除。 |
| S06 | .omo全部Markdown证据/notepads/HANDOFF、superpowers交接issues/learnings/problems/README、三份draft | 历史报告按所指模块与现行生产链对照；不以旧二进制或旧测试名代替源码 | 证据仅对原提交有效；未验收事项保留第六节。重复归档非新要求，draft不是已批准决定，原始非Markdown日志不在逐字覆盖集合。 |

## 8. 文档覆盖清单

完整来源清单已覆盖1110个路径，包含原234来源及新增876来源；CLAUDE.md是AGENTS.md别名，仅同SHA正文共享阅读。下表保留原142个跟踪来源的历史映射，后续87份工作记录和本轮忽略目录/历史分支来源统一见 [覆盖索引](coverage-index.md) 与 [完整指纹索引](hidden-review/expanded-source-index.json)。不把旧表行数当作当前全部来源数量。
覆盖根文档、docs全部ADR/规范/历史specs/plans、.omo计划/交接/notepad/Markdown审查记录、Web/性能/移动QA说明及PR模板。
完整读取记录来自上轮全文批次及本轮补充对照；不把上轮“未发现”自动升级成全部断言已经证明。
不包含依赖/构建产物、未跟踪.worktree副本、原始TXT/JSON日志及参考HTML的逐字审查。
原始证据按需要追查；以下清单固定到源码基线，不含本次新增报告自身。

| 文档路径（仓库根目录相对） | 对照族 / 阅读边界 |
|---|---|
| `.github/pull_request_template.md` | S02：工程/架构/协作；全文读取 |
| `.omo/HANDOFF.md` | S06：历史交接/证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/compatibility-removal.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/notepad-recovery.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-1-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-10-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-11-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-12-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-13-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-14-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-15-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-16-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-17-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-19-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-2-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-20-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-21-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-22-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-24-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-25-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-26-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-27-manual-continuation.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-27-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-28-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-29-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-3-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-30-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-33-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-34-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-36-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-4-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-7-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-8-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/task-9-review.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/company-information-npc-intentions/worktree-baseline.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/escrow-parallel-engine/F3/manual-qa/README.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/escrow-parallel-engine/task-10/divergence-audit.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/escrow-parallel-engine/task-11/execution-log.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/escrow-parallel-engine/task-12/validation.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/escrow-parallel-engine/task-3-8-smoke.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/escrow-parallel-engine/task-3/d6-d7/README.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/escrow-parallel-engine/task-3/d6-d7/structured-comparison.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/escrow-parallel-engine/task-8/acceptance-map.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/escrow-parallel-engine/task-9/corpus-diff.md` | S06：历史验收证据；全文读取 |
| `.omo/evidence/escrow-parallel-engine/task-9/historical-witness-audit.md` | S06：历史验收证据；全文读取 |
| `.omo/notepads/company-information-npc-intentions/decisions.md` | S06：历史交接/证据；全文读取 |
| `.omo/notepads/company-information-npc-intentions/issues.md` | S06：历史交接/证据；全文读取 |
| `.omo/notepads/company-information-npc-intentions/learnings.md` | S06：历史交接/证据；全文读取 |
| `.omo/notepads/company-information-npc-intentions/problems.md` | S06：历史交接/证据；全文读取 |
| `.omo/plans/company-information-npc-intentions.md` | R14；全文读取 |
| `.omo/plans/escrow-parallel-engine.md` | R02；全文读取 |
| `.omo/plans/resolve-blockers-wayland.md` | R20；全文读取 |
| `AGENTS.md` | S02：工程/架构/协作；全文读取 |
| `CLAUDE.md` | AGENTS.md 别名，正文不重复计数 |
| `CONTRIBUTING.md` | S02：工程/架构/协作；全文读取 |
| `DESIGN.md` | R17；全文读取 |
| `README.md` | S02：工程/架构/协作；全文读取 |
| `UX-CONTRACT.md` | R17；全文读取 |
| `apps/web/README.md` | S02：工程/架构/协作；全文读取 |
| `design/ui/mobile/qa/README.md` | R17；全文读取 |
| `docs/actions-cache.md` | S03：部署发布；全文读取 |
| `docs/architecture.md` | S02：工程/架构/协作；全文读取 |
| `docs/build-and-deployment.md` | S03：部署发布；全文读取 |
| `docs/causal-diagnostics.md` | S04：验收/诊断；全文读取 |
| `docs/ci-build-fixes.md` | S03：部署发布；全文读取 |
| `docs/company-accounting.md` | R03；全文读取 |
| `docs/company-actions-design.md` | R03；全文读取 |
| `docs/decisions/0000-template.md` | S02：工程/架构/协作；全文读取 |
| `docs/decisions/0001-record-architecture-decisions.md` | S02：工程/架构/协作；全文读取 |
| `docs/decisions/0002-engine-rust-wasm.md` | S02：工程/架构/协作；全文读取 |
| `docs/decisions/0003-backend-rust.md` | S02：工程/架构/协作；全文读取 |
| `docs/decisions/0004-frontend-state-redux-toolkit.md` | S02：工程/架构/协作；全文读取 |
| `docs/decisions/0005-unified-engine-three-deployments.md` | R19；全文读取 |
| `docs/decisions/0006-npc-strategy-module.md` | R04；全文读取 |
| `docs/decisions/0007-three-deployment-frontend-framework.md` | R19；全文读取 |
| `docs/decisions/0008-gpu-and-compute-offload.md` | R18；全文读取 |
| `docs/decisions/0009-call-auction-and-intraday-axis.md` | R06；全文读取 |
| `docs/decisions/0010-unified-host-protocol-and-local-refresh.md` | R19；全文读取 |
| `docs/decisions/0011-market-time-observations-and-position-risk.md` | R05；全文读取 |
| `docs/decisions/0012-retail-observation-to-target-position-loop.md` | R05；全文读取 |
| `docs/decisions/0013-retail-experience-memory.md` | R05；全文读取 |
| `docs/decisions/0014-closing-call-auction.md` | R06；全文读取 |
| `docs/decisions/0015-parent-order-execution.md` | R06；全文读取 |
| `docs/decisions/0016-fundamental-factor-model.md` | R03；全文读取 |
| `docs/decisions/0017-escrow-parallel-tick.md` | R02；全文读取 |
| `docs/decisions/0018-long-running-immutable-timeline.md` | R01；全文读取 |
| `docs/decisions/0019-draft-market-scope-and-capacity.md` | S01：现行领域边界；全文读取 |
| `docs/decisions/0020-native-allocator-for-concurrent-ticks.md` | R18；全文读取 |
| `docs/decisions/0021-strategy-position-choice-and-noise-pricing.md` | R04；全文读取 |
| `docs/decisions/0022-symbolic-limit-prices.md` | R16；全文读取 |
| `docs/decisions/0023-synthetic-history-and-matching-only.md` | S01：现行领域边界；全文读取 |
| `docs/decisions/0024-shrinking-investor-cash-pool.md` | S01：现行领域边界；全文读取 |
| `docs/decisions/0025-day-end-only-persistence.md` | R01；全文读取 |
| `docs/decisions/0026-individual-institution-experience.md` | R04；全文读取 |
| `docs/decisions/0027-runtime-deployment-and-build-targets.md` | S03：部署发布；全文读取 |
| `docs/decisions/0028-tagged-release-and-static-pages.md` | S03：部署发布；全文读取 |
| `docs/diagnostics.md` | S04：验收/诊断；全文读取 |
| `docs/error-handling.md` | S02：工程/架构/协作；全文读取 |
| `docs/git/AGENTS.md` | S02：工程/架构/协作；全文读取 |
| `docs/git/daily-workflow.md` | S02：工程/架构/协作；全文读取 |
| `docs/git/initialization.md` | S02：工程/架构/协作；全文读取 |
| `docs/implementation-gaps.md` | S05：范围与进度；全文读取 |
| `docs/naming-conventions.md` | S02：工程/架构/协作；全文读取 |
| `docs/naming-refactor-validation.md` | S04：验收/诊断；全文读取 |
| `docs/open-questions.md` | S05：范围与进度；全文读取 |
| `docs/price-volume-simulation-gap-checklist.md` | R16；全文读取 |
| `docs/principles.md` | S02：工程/架构/协作；全文读取 |
| `docs/roadmap.md` | S05：范围与进度；全文读取 |
| `docs/simulation-calendar.md` | S01：现行领域边界；全文读取 |
| `docs/superpowers/2026-09-13-company-information-archive.md` | R20；全文读取 |
| `docs/superpowers/README.md` | S02：工程/架构/协作；全文读取 |
| `docs/superpowers/plans/2026-06-29-account.md` | R07；全文读取 |
| `docs/superpowers/plans/2026-06-29-initial-positions.md` | R12；全文读取 |
| `docs/superpowers/plans/2026-06-29-market.md` | R09；全文读取 |
| `docs/superpowers/plans/2026-06-29-money-fixed-point.md` | R13；全文读取 |
| `docs/superpowers/plans/2026-06-29-orderbook.md` | R08；全文读取 |
| `docs/superpowers/plans/2026-06-29-session.md` | R10；全文读取 |
| `docs/superpowers/plans/2026-06-29-strategy-impl.md` | R11；全文读取 |
| `docs/superpowers/plans/2026-09-10-company-information-npc-intentions.md` | R14；全文读取 |
| `docs/superpowers/plans/2026-09-13-resolve-blockers-wayland.md` | R20；全文读取 |
| `docs/superpowers/plans/2026-09-24-single-world-multithreading.md` | R15；全文读取 |
| `docs/superpowers/plans/2026-09-25-production-entry-and-thread-pool.md` | R15；全文读取 |
| `docs/superpowers/plans/2026-09-26-ready-receipt-and-thread-boundary.md` | R15；全文读取 |
| `docs/superpowers/plans/2026-09-26-sparse-continuous-book-feedback.md` | R16；全文读取 |
| `docs/superpowers/specs/2026-06-29-account-design.md` | R07；全文读取 |
| `docs/superpowers/specs/2026-06-29-gameconfig-design.md` | R12；全文读取 |
| `docs/superpowers/specs/2026-06-29-initial-positions-design.md` | R12；全文读取 |
| `docs/superpowers/specs/2026-06-29-market-design.md` | R09；全文读取 |
| `docs/superpowers/specs/2026-06-29-money-fixed-point-design.md` | R13；全文读取 |
| `docs/superpowers/specs/2026-06-29-orderbook-design.md` | R08；全文读取 |
| `docs/superpowers/specs/2026-06-29-session-design.md` | R10；全文读取 |
| `docs/superpowers/specs/2026-06-29-strategy-impl-design.md` | R11；全文读取 |
| `docs/superpowers/specs/2026-09-11-company-information-handoff.md` | S06：历史交接/证据；全文读取 |
| `docs/superpowers/specs/2026-09-13-company-information-issues.md` | S06：历史交接/证据；全文读取 |
| `docs/superpowers/specs/2026-09-13-company-information-learnings.md` | S06：历史交接/证据；全文读取 |
| `docs/superpowers/specs/2026-09-13-company-information-problems.md` | S06：历史交接/证据；全文读取 |
| `docs/tech-stack.md` | R18；全文读取 |
| `docs/test-cleanup-checklist.md` | S04：验收/诊断；全文读取 |
| `docs/testing.md` | S04：验收/诊断；全文读取 |
| `docs/trading-rules.md` | S01：现行领域边界；全文读取 |
| `docs/work-status.md` | S05：范围与进度；全文读取 |
| `scripts/performance/README.md` | S04：验收/诊断；全文读取 |
| `.omo/drafts/resolve-blockers-wayland.md` | S06：未跟踪历史draft；全文读取、非已批准决定 |
| `.omo/drafts/k7-deterministic-multicore-utilization.md` | S06：未跟踪历史draft；全文读取、非已批准决定 |
| `.omo/drafts/escrow-parallel-engine.md` | S06：未跟踪历史draft；全文读取、非已批准决定 |
