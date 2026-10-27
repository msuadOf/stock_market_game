# 公司六项只读查询的三宿主注册

## 最新状态（2026-10-28）

engine 首红十例已真实完成并由未实施 Sol 终核：九个指定 query 业务断言红、一个严格请求绿，构建与执行约 92.285 秒，893 项源码前后一致，已观察进程及 scratch 收口。实际 red／prior 单件均已封存，原件见 [engine-red-01](engine-red-01/)。

六项只读 dispatch、Server 当前 membership／HTTP 注册、Tauri actor／IPC 注册、WASM registry／export 代码已落在本地候选 `cef172e6`，十四产品路径完整 diff 获两名未实施 Sol 的源码预审；首次实际四包编译暴露该预审漏掉的 Tauri 类型错误，不能将静态结论视为编译通过。

首次四包批次 `four-host-green-01` 的 session `32018` 实际退出 `1`，耗时 92.326 秒：`lookup_handles` 返回 `SendCommandError`，而新增 `CompanyReadIpcError::Session.message` 需要 `String`，在 Tauri `lib.rs:240` 发生 E0308。没有执行十八个 case，也没有产生 wire fixture。三个已编 unit 仅作为 partial artifact 在原共享截止内独立封存；观察到的进程与 scratch 已清理，编译时段已交 root 指定的 E2。

重试准备时，`green-candidate-v3` 仅将该错误显式转为 `error.to_string()`，保留具体原因；六项 query、owner 与领域算法未改。`four-host-green-02` 保持 1001 项输入／8 项 runner、唯一 RAM target、j8／INC0／原 debug2、十八个短 exact、共享 297／299／300 秒绝对截止，仅更换本批输出目录；当时 slot 与实际前槽身份 pending，等待 root 在 E2 之后重新分配，未提前 Cargo。成功后的真实 engine／Server HTTP／Tauri IPC JSON 计划交 B/C 消费，WASM 真实 export／浏览器仍由 B/C／W1 实际制品门验证。

随后 root 正式分配 green02，实际绑定 E2 session `72913` 的新 default unit `21ffb460…`，并在本批共享窗先独立封存。green02 session `57868` 实际退出 `0`，四包编译及十八个 exact 全部通过，耗时 19.355659 秒，最长单 case 命令 1.824 秒；1001 项来源始末一致，观察到最多二十六线程、每 500ms 最多 301 CPU ticks。WASM／Server／engine 三个同源 partial 由 Cargo 合法报告 fresh true、SHA 与失败批相同；Tauri 修后真实重新编译，fresh false。四包 unit 均独立封存，owned／argv 数组为空、scratch 已清理；时段已交 root 指定 R4red03，没有连跑。

真实 stdout 已提取 engine／Server HTTP／Tauri mock IPC 各六项 JSON 和一项 Tauri typed 领域失败，见 [green02 执行证据](four-host-green-02/)，已通过 followup 交 B/C 继续消费验证。两名未实施 Sol 已分别完成实际 runner 终核和产品／执行证据终核，十八门、源码／制品／fixture 绑定及清理均通过，详见 [实际终审](four-host-green-02/actual-review.md) 与 [分层交接](handoff.md)。该结果限定于 Rust 四包 native 门；WASM export／浏览器、TS 跨层消费及生产公司结算不在本次通过范围。本地 main 接收等待 E2 公共 base 出口，不另造重复全宿主回归。

## 准备过程（保留历史）

截至本记录写入：只有测试、显式 stub、接口 enum 与设计审查记录；尚未运行 Cargo，尚未取得实际红／绿，尚未完成三宿主注册，也未交付真实 wire fixture。root 独占 RAM target 的队列正在处理其他批次。本任务不自行启动 Cargo／metadata。

root 随后明确授权在同一未执行 stub 上补两个 owner 非空与 CompanyId／StockId 不相等正控，首红从七例扩为十例，避免为漏覆盖重复构建。`red-*-v3.rs` 为新快照；旧七例准备保存在 `engine-red-01/prepared-seven-case-v1/`，所有这些都尚未实际执行。当前 static 目标为九红一绿。

代码树从可信 main `c10cf187` 创建。测试／stub 先本地提交保存，再 rebase 到已实际完成集成的可信 main `8e5b32de`；当前提交 `f305495b`。`pre-base-align.diff` 与 `red-*-v1.rs` 保留准备过程，v1 含未静态修正的 fixture 构造，不是实际测试结果。`red-*-v2.rs` 是申请首红时的源码快照，仍不是运行结果；include 相对路径按生产测试文件位置解释，不能在工作记录目录直接编译。

## 最小文件／接口范围

| 层 | 文件 | 目的 |
|---|---|---|
| engine | `session/protocol.rs`、`protocol/company_reads.rs`、`protocol/company_reads/tests.rs` | 六项 query 的共同只读 dispatch、真实结果 enum、健康与身份／目录边界及 typed 错误 |
| Server | `actor.rs`、`actor/fatal_tests.rs`、`routes.rs`、`routes/auth_tests.rs`、`lib.rs` | 当前 member／generation 校验、actor 回执与实际 HTTP 注册 |
| Tauri | `actor.rs`、`actor/protocol_tests.rs`、`lib.rs` | 本地 owner 0、generation、actor 回执与实际 invoke 注册 |
| WASM | `lib.rs`、`protocol_tests.rs` | 实际 session registry 与导出绑定，本地 owner 0 |

不修改 `GameSession` 的六项领域 query、税算法、资金／股份结算、运行时中央编排或 TS `EngineHost`。TS 由原 B/C owner 独占。

## 输入与成功外壳

- engine request：serde adjacent `{kind,data}`。unit query 无 data；公司解释 data 为 `{company: CompanyId,period_end: CivilDate}`，能力 data 为 `{company: CompanyId}`。严格拒绝未知字段，account／owner 不属于请求。
- kind：`CompanyPeriodExplanation`、`AccountDividendTaxStatus`、`DividendTaxOutstandingViews`、`OwnerFlatWithholdingReceipts`、`CompanyCapabilities`、`OwnerRightsOfferings`。
- 结果：同名 kind 的 data 为既有领域类型原样 serde。股息税／配股中的证券沿 `StockCode` 字符串，不当作 `StockId`。`Money` 分字符串、`AccountingAmount` 元字符串及股数／bp 沿原类型，不换单位。
- Server／Tauri 成功：`{generation: string,owner: AccountId,value: {kind,data}}`；WASM 成功：`{owner: AccountId,value: {kind,data}}`。AccountId 沿实际规范十进制字符串 serde。
- 六项新增接口均要求当前 member；不改变原公开报告接口的观战权限。Server 在 actor 内由当前 subject 的 membership 导出 owner；本地固定 player 0。空数组也包含真实 owner。

错误投影的必要性与安全界限见 [设计审查记录](design-review.md)。最终 error enum 源码及真实 serde fixture 要等实际首红后实现并冻结，不能把这里的计划当作已注册 API。

## 首红资源准备

[engine-red-01](engine-red-01/) 仅准备 engine 十个短 case 的一次长构建与并发 exact，复用已审 P1 runner／guard／child collector。slot 与前槽实际 binary 身份均 pending，尚不授权启动。静态预测为九个 stub 行为红、一个严格请求绿；实际分类必须逐项消费 child exit、summary、指定断言位置、明确 stub 内容与源码／制品绑定。执行／清理发布／外层分别共享同一 anchor 的 297／299／300 秒绝对截止，新增 batch 局部 absolute supervisor 不改共享 helper。

不得将 engine 首红称为三宿主、浏览器 WASM 或生产公司结算已经完成。完整 diff 的未实施审查仍为必需门禁。
