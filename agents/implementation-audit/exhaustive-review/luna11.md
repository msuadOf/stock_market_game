# luna11：ADR-0023–0025 全文实现复核

## 范围与读取

- 目标代码基线：`08e4fc75b52a71a3262a8a938c57b44f8b5b4960`；复核时 HEAD 为 `a7c7ce357bdc9f88c03633744b2d5815db49e9b2`。两者之间产品代码没有差异；`git diff --name-only 08e4fc7..HEAD` 所列10项均为 `agents/implementation-audit/` 既有复核记录。因此结论针对指定基线产品代码，不把后续文档整理算成产品实现。
- 已连续读至 EOF：根 `AGENTS.md`、`docs/principles.md`；ADR-0023 61行、ADR-0024 53行、ADR-0025 55行，共169行。逐条映射采用这些文件原始行号。另完整读过既有 `sweep11.md`，并独立追生产调用链，不以旧审查结论替代代码核对。
- 仅做静态读取与检索；未改产品代码、未运行测试或长验收。A股交易制度没有新增或改写；费用核对沿用 `docs/trading-rules.md:41-48` 登记的游戏简化，不能表述为真实券商清算规则。

## 条款矩阵

| 决策条款 | 生产代码追踪及判断 | 旧核销复核 |
|---|---|---|
| ADR-0023 §15–16（原文15–16）：360根负时间虚拟日K、seed隔离、`trade_stats=None` | `packages/engine/src/session/candles.rs:208-265` 固定360根；每股使用 `seed ^ stock_code_hash ^ salt` 的局部 `SplitMix64`；时间从 `-86400` 至 `-360*86400`，前史量价不伪装成本局逐笔统计。`session.rs:1376-1426` 新局装入前史，随后才建立会话 RNG。 | 旧结论“合成前史已实现”仍成立；不是实盘数据校准，也未发现外部历史导入路径。 |
| ADR-0023 §17–18（原文17–18）：day=0 首日实际成交、价格/量/额/笔数来自成交 | 连续成交走 `continuous_tick_finalizer.rs:371-412`，先验证正价、成交额乘法、量额计数溢出，再从成交事实更新日K；收盘竞价走 `auction_day_end.rs:1125-1138`，逐个实际 match 更新。成交汇总实际成交额为 `price * qty`（分×股），笔数按实际撮合记录增加。未见运行期随机K线替代此链。 | 旧结论“实际受理与撮合生成日K”仍成立。不能从这些生产投影推导逐笔历史也被永久保存。 |
| ADR-0023 §19–23（原文19–23）：day=0时间为0；零成交占位、首笔成交替换OHLC | `candles.rs:131-179` 使用 `day * 86400`；无成交归档保留零量、零统计，首笔正量成交重置OHLC，后续成交累计量额笔数；收盘无撮合时以昨收记录零量（`auction_day_end.rs:1131-1137`）。`candles.rs:362-373,419-435` 有对应测试源码。 | 旧结论日界及零成交无假量仍成立；本轮未运行测试。 |
| ADR-0023 §24–27、§31–39（原文24–27、31–39）：C06不适用、旧密封证据不改写、不开局导入真实前史 | 政策验证器与 runner 的 v8/`not_applicable_synthetic_history_only` 接线已由 `sweep11.md` 追到工具行；产品启动生成入口为 `session.rs:1364-1377`，使用 setup 与 seed。没有新实现义务要求真实行情或将历史数据重新标记为校准。 | 旧结论不扩大证据：策略/分布长期矩阵未因政策字段而变成通过；无新代码遗漏。 |
| ADR-0024 §20–25（原文20–25）：允许投资者现金池缩小、不造钱/对手盘、零成交合法 | 实际结算只消费 `ReceiptKind::Fill` 且数量为正的收据（`settlement.rs:54-93`）；结算金额来自已审核收据增量（`:151-177`）。零成交收盘路径仍生成零量K，不造交易。限定的结算与现金转移路径中未发现 top-up、NPC收入、费用返还或资金重置。 | 旧结论“不需要资金循环”成立；静态检索不声称全仓数学证明不存在任何现金来源，但公司经营款项另有账簿 owner。 |
| ADR-0024 §26–31（原文26–31）：真实资金费用校验；经营与投资者现金隔离；Q12核销 | 买入结算检查成交额+佣金+过户费是否超过现金，超出显式返回 `InsufficientCash`（`account.rs:326-343`）；卖出按真实receipt数量/金额入账。卖方实收受本次成交款封顶且按佣金→印花税→过户费分配（`transition.rs:189-234`）；收入可能因此低于名义费用，属于已登记游戏简化，不是透支或免收。公司日结经营另经 `ops_wiring`（`session.rs:2100-2128`），不自动给投资者账户注资。 | 旧结论成立。G35的公司经营付款缺口不等同投资者补钱需求；没有重复生成新缺口。未重新核验法规费率，依据只限已登记规则范围。 |
| ADR-0025 §12–14（原文12–14）：完整自然日日结后才持久化；含经营、封账、披露及休市日 | 引擎先要求交易会话数与CivilClock同步（`session.rs:2073-2097`），再跑到期经营、封账、披露（`:2100-2128`）。`ProtocolSession::end_civil_day_update` 成功完成CivilUpdate校验后才 `game.save()` 并替换候选（`protocol/civil/session.rs:283-329`）；任一失败回滚 checkpoint（`:314-319`）。宿主按 `civil_day_ready` 循环自然日结（Server `actor.rs:1199-1211`；Desktop `actor.rs:1163-1175`）。 | 旧结论成立；G35经营内容债仍是单独项目，不由候选存档接线核销。 |
| ADR-0025 §15–18（原文15–18）：日内无公共保存；首次日结前报错；内存checkpoint/活动委托查询分离 | `ProtocolSession::save` 只返回 `day_end_save`，首个日终前显式错误（`protocol/civil/session.rs:175-184`）；checkpoint/rollback只复制内存状态（`:71-80`）。保存候选不是当前live snapshot。活动委托独立查询在 Web Remote/Tauri host，既有 `sweep11.md` 行号指向其 Server/Desktop查询实现。 | 旧结论“内存检查点不是持久档”成立；不应因底层 `GameSession::save()` 存在就复活日内存档要求。 |
| ADR-0025 §19–22（原文19–22）：不可变候选、三宿主保存同一候选、目标复用 | CivilUpdate成功后以 `Arc<SaveSlot>` 同时更新最新日终档及按seq索引的候选（`protocol/civil/session.rs:321-329`）；指定候选要求seq及settled date一致（`:186-213`）。Web收到CivilUpdate即捕获引用并请求候选（`App.tsx:234-245`），写操作经 `DayEndPersistence` 串行化、generation检查（`day-end-persistence.ts:20-36`）。WASM worker (`wasm-worker.ts:200-212`)、Server (`server/actor.rs:1342-1356`)、Desktop (`desktop/src-tauri/src/actor.rs:1027-1039`) 均区分指定候选与普通保存；候选不存在/失效会返回错误。 | 旧结论成立。Server无订阅者会主动丢弃待取候选（`server/actor.rs:1240-1245`），与没有Web消费者时不保存的宿主生命周期一致，不是静默回退到live state。 |
| ADR-0025 §21–25（原文21–25）：快速槽/授权文件日终更新、错误可见、旧异步响应不能覆盖 | Web仅CivilUpdate分支触发日终协调（`App.tsx:234-251`）；快速槽及选定文件目标分别写，多个目标状态经 `allSettled` 聚合并显式报告（`day-end-targets.ts:6-22`）。LocalStorage在压缩后检查目标generation再写（`save-repository.ts:70-78`）；浏览器文件写后close，失败abort（`save-file.ts:151-170`）；Tauri写同目录随机临时文件再rename（`:61-100`）。旧局/读档/新局会invalidate并等待在途操作（`useSaveCommands.ts:63-74,95-123,145-207,211-238`）。 | 旧结论“显式失败与目标隔离”仍成立。多目标非分布式原子：一端成功另一端失败会明确报告部分失败，ADR未要求跨目标回滚。 |
| ADR-0025 §26–29（原文26–29）：启动快速槽只读一次；坏档拒绝且允许新局/换档 | `InitialSaveSource.read` memoize pending并在完成后标记消费（`session-replacement.ts:1-24`）；启动先取存档setup/seed、再建host、加载完成后 `host.start`（`useSessionHostLifecycle.ts:90-105,178`）。其后读档仅由明确命令执行（`useSaveCommands.ts:95-123`）。错误页提供明确新局/另选日终档按钮（`App.tsx:416-429`）。 | 旧结论成立；普通新局固定默认seed仍是G20既有事项，不是存档读取遗漏。 |
| ADR-0025 §30–36（原文30–36）：公共加载仅完整日结档、拒绝活动订单/资源、保留跨日个人事实 | Rust公共恢复先深度 `GameSession::restore`，再校验已结算自然日、tick/day一致及无活动连续/竞价委托、envelope、母单、生命周期、待受理请求（`protocol/civil/session.rs:100-140`）。Web浅筛同类事实（`day-end-candidate.ts:6-27`），最终宿主仍经过Rust公共入口。现有SaveSlot精简/校验保留日终跨日计划与个人状态；本次不要求将日内订单放回公共档。 | 旧结论成立；“改日终现金后原日内冻结买单如何重算”经决策排除，不是遗漏。 |
| ADR-0025 §38–55（原文38–55）：实施边界、严格新格式、短测边界 | 严格schema版本拒绝与无旧格式迁移按既有 `sweep11.md` 及 `persistence/v2.rs` 路径复核；验证条款记述的是当时的定向短测范围，不是全量或本轮运行结果。 | 旧结论没有把未运行矩阵冒称通过；保留此边界。 |

## 旧结论复核与候选反证

- `sweep11.md` 将 R01/S01 的“前史、真实成交量价、可缩小资金池、日终不可变候选、公共日级恢复隔离”核销为已有主干。本次按生产 owner 与消费者重新追踪，以上核销仍有实现依据；没有发现新的已证实撮合、资金或日终存档语义遗漏。
- 逐项排除的假阳性：随机量不是实际成交量（它只存在于负时间合成前史）；占位K无成交统计；成交费用退出会减少参与者现金，但不代表错误资金泄漏；内存检查点和活动委托查询不是持久写入；CivilUpdate之后异步落盘仍持有原seq/date候选；日内父委托移除不等于跨日计划丢失。
- **未确认候选（需针对Windows核实）：** Desktop文件更新用 `tauri-plugin-fs 2.4.5` 的 `rename(temporaryPath, path)` 覆盖现存目标（`apps/web/src/save/save-file.ts:78-80`）。Rust/Windows的rename覆盖语义有平台差异可能，项目有Windows Desktop配置（`apps/desktop/src-tauri/Cargo.toml:19`）。若插件底层在Windows目标已存在时拒绝替换，授权文件首日可写而下一日无法更新，将违反ADR-0025 §21。当前静态证据不足以确认插件底层行为或复现，不能作为确定缺口；需在Windows对预先存在目标做一次原生替换验证，失败才登记修复。
- **未发现其他新候选：** WebFS写入在close前错误时尝试abort；Tauri写入先写临时路径，源或替换失败都会显式报错并清理；权限撤销、进程断电和文件系统故障没有被本轮实测。它们是尚未实测边界，不可误报为已证实缺陷，也不代表长验收通过。

## 复核结论

在指定基线可读代码范围内，旧总账对ADR-0023至0025的生产接线核销大体成立；没有发现已证实的新交易语义或持久化实现遗漏。唯一值得专项反证的是Tauri Windows覆盖现存目标的跨平台行为，当前列为未确认候选。本文为静态审计，未运行测试、未核验Windows运行结果，也不代替后续独立复核。
