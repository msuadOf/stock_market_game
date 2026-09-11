# Task 16 独立复核 — 将公共曝光与NPC本人获取信息分离

VERDICT: APPROVE

- 复核对象：commit `2d15e7c` (`feat(engine): 隔离公开曝光与个人已知信息`)，branch `codex/feat/web-ui-polish`，parent `da6c399`（task-15，已 APPROVE）。
- 复核人：独立 subagent（未实施本改动）。AGENTS.md「大A语义与独立复核门禁」第 7 次强制执行。
- 隔离环境：`git worktree add C:\Users\msuad\AppData\Local\Temp\opencode\wt-review-16 2d15e7c`（ detached）。主树有 task-25 zombie 未提交工作（tests/attention_discovery/ 等），**未在主树运行任何 cargo / git 写操作**；主树仅做只读 git/文件读取。
- 原始输出：`C:\Users\msuad\AppData\Local\Temp\opencode\t16r-{1,2,3,4}.txt`（cmd /c 重定向）。
- 提交面：8 文件全部 `packages/engine`（src 3：acquisition.rs 284 raw / npc_view.rs 115 / information/mod.rs +11；tests 5：fixture 322、acquisition_gold 242、failures 256、view_gold 184、main 17），零 docs/fixture-policy 改动，1431 insertions。

---

## 一、AGENTS.md 三问

### Q1 是否符合大 A 语义，依据是否可靠？

**符合。** 本任务是游戏内部信息架构（NPC 个人获知 vs 公共曝光），不新增任何真实 A 股规则的简化声明；其领域基线是用户批准的计划 K4 锚（plan L121–123，逐字核对）：

| K4 锚（plan 原文） | 实现落点 | 测试锁定 |
|---|---|---|
| 「NpcInformationState 逐公司记录获得的报告/公告ID及观察时间」 | `NpcInformationState { owner: AccountId, companies: BTreeMap<CompanyId, Vec<AcquisitionRecord{id, observed_at, kind}>> }`（acquisition.rs L110–116） | `state_serde_round_trip_preserves_acquisitions`（含 per-company 记录数断言） |
| 「公共曝光仅改变发现机会…借助公共索引定位候选后，只有实际观察才将内容纳入个人信息集」 | `discovery_candidates(library, company, as_of)` = 纯 id 投影（as_of 过滤 + id 序），候选≠阅读；获知必须走 `record_acquisition` | `discovery_candidates_index_publications_without_reading`（公布前空 / v1 后单候选 / 全部后三候选 id 序 + 空状态 ⇒ NotAcquired） |
| 「未观察NPC不能直接读最新全部公告」 | `NpcObservationContext::report/announcement` 先 `require_acquired` ⇒ `NotAcquired` 类型化拒绝 | `unacquired_read_rejected`、`unread_publication_never_enters_unread_npc_inputs` |
| 「观察是在现有候选注意力被接受时发生」 | 注意力接线显式移交 task 25（模块文档 + notepad 登记）；本 diff 不触 session/attention/strategy | 无提前接线：extraction_replay 3/3 绿（见 Q3） |
| 「休市日不…偷偷全体阅读」 | 无任何批量/日终/全体阅读 API；`record_acquisition` 显式单条、纯 civil 瞬间、无 RNG/时钟 | 结构性保证（diff 内无 calendar 驱动路径） |
| 「dev查看未公开数据不得写入NPC信息状态」 | dev/宿主查看面 = 任务15 公开库查询 + 候选索引，全 `&self`；未公开数据经库面 EarlyRead 拒绝（任务15 守卫） | `dev_reads_leave_every_npc_state_unchanged`（双 NPC 状态字节 + 判定投影前后不变） |
| 「新报告/更正不会追溯重写…当时看过的材料」（L123） | 更正=新 id + 读取 as_of=首次获知时点 ⇒ 版本钉死，零内容副本 | `acquired_version_pinned_across_later_correction`（v1 字节逐位不变跨 v2 公开；v2 需新获知；获知 v2 后 v1 仍不可变） |

时间/金额数学全部整数（CivilInstant 秒、PublicationId u64、无任何 f64）；serde 确定性（BTreeMap 公司序 + 公司内 id 严格递增 Vec，往返字节一致断言）。

### Q2 改动是否为需求所必需、是否保持最小范围？

**是。** 计划任务 16 实施面 = `information/{acquisition,npc_view}.rs` + `NpcObservationContext`；实际恰好如此 + mod.rs 11 行接线 + 5 个测试文件。无 strategy/session 抢先接线（task 25/26 职责）、无多余抽象（无请求结构体、无 trait、无 fallback 层）。守卫透传复用任务 15 查询守卫（`resolve_publication` 报告面→公告面→EarlyRead 原样透传），不重复实现。NPC 身份复用 `AccountId`，不建平行身份。

### Q3 是否存在遗漏的边界测试、跨层语义漂移或不必要复杂度？

**无阻断项。** 13 测试（金样 6 + 拒绝 7）覆盖计划 QA 全部条目（见下节映射）。跨层无漂移：engine-only、无 session/web/server 改动、information 类型未加 ts_rs（沿任务 15 决定，任务 29 收编）、extraction_replay 在隔离全量中 3/3 绿 ⇒ RNG/存档锚零漂移。复杂度最小。三条非阻塞覆盖备忘见「观察」节（供任务 18/25/26 消费者）。

---

## 二、验收/红线逐项核验（计划 K4/K5 + 任务16 Acceptance + QA failure 清单）

**Acceptance（大项）——「改变未披露事实、或已披露但本人未读的内容，不改变该NPC观察前判断」：**
`unread_publication_never_enters_unread_npc_inputs` 构造受控场景：(a) 开放期间过账未披露分录（`post_undisclosed_fact`：2031-02 现款收入，只改总账、不结账不公布）⇒ 乙（未读）判定输入字节投影不变；(b) 更正 v2 公开但本人未读（`publish_correction`：结账重述→v2 supersedes v1→Q1 相位公开）⇒ 乙投影不变且 v2 NotAcquired。**区分力对照**：甲真实获知 v2 后投影必变（防空洞测试）。`acquired_version_pinned_across_later_correction` 补上有获知存量的甲侧不变性（更正公开不动甲的任何输入）。**逐人延迟获知**：乙更晚首读年报 v1 ⇒ 只动乙本人输入。**一次获知只记一次**：`repeat_acquisition_is_recorded_once`（Recorded→AlreadyAcquired{first_observed_at=首次}、状态 serde 字节跨重复不变）。**dev查看不算NPC阅读**：见 Q1 表。判定投影 = 已知 id + 获知时点 + 内容 serde 字节（报告前公告后，公司序+id 序）——是「观察前判断输入」的忠实代理。

**NpcObservationContext 类型面**：四输入构造（npc、&NpcInformationState 属主校验、&PublicLibrary、&Market 泛型）；字段/访问器面零 CompanyState/Books/总账路径；内容按引用暴露（`&'a PublishedReport`）。`context_surface_only_consumes_information_state_and_library` 以最小构造钉死。Market 泛型 = session（任务26）装配可见行情，引擎信息域不依赖 session——合理的边界（最终由任务 26 实例化决定，登记于模块文档+learnings）。

**内存经济**：状态 serde 形状 = `{owner, companies: {company: [{id, observed_at, kind}]}}`——只存 id+时点+种类，零报表体克隆；上下文全引用（往返测试断言字节即证）。

**QA failure 清单映射**（同命令 `--test information_acquisition`）：
- 未来 publication 拒绝：`future_report_publication_rejected`（EarlyRead 携带 id+published_at+as_of 完整上下文字段绑定断言；拒绝后状态零记录）+ `future_announcement_publication_rejected`（公告面）。
- observed_at 早于 published_at：与「未来 publication」是同一 EarlyRead 守卫的两种表述（实施者 notepad #3 已登记澄清）——报告面/公告面各锁一例，守卫条件即 `observed_at < published_at`。裁决：接受（一项不变量两表述，双面覆盖 + 字段级断言强于字面拆分）。
- 未知 report ID 拒绝：`unknown_publication_rejected`（UnknownPublication 透传，id 绑定断言）。
- 他人信息集注入拒绝：`cross_npc_injection_rejected`——**双面**（登记面 `record_acquisition` OwnerMismatch 且零记录 + 上下文构造面 `NpcObservationContext::new` OwnerMismatch）；乙自己状态照常可用且为空（隔离无泄漏）。
- 附加：篡改存档 `tampered_restore_rejected`（公司内重复 id / 非严格递增 / 跨公司重复 ⇒ InconsistentState，from_parts 与 serde 边界双锁）。

**接线范围**：mod.rs 仅 +11（2 个 mod 声明 + 2 个 pub use + 模块文档）；strategy/session 零触碰；隔离全量 extraction_replay `identical_construction_replays_bit_identical` 等 3/3 绿 ⇒ 无 RNG 流移位。

**LOC 天花板**（纯行口径：非空/非注释/非属性/非 use，本人独立复数）：
- acquisition.rs raw 284 / 纯 193（实施者口径 208）≤ 250 ✓（raw 284 已在任务指令预核）。
- npc_view.rs raw 115 / 纯 75（实施者 80）✓。
- fixture.rs raw 322 / 纯 254（实施者 265）——**实施者已按数据表例外登记**（~110 行分录/公布请求结构体字面量；task-1 baseline_fixture / task-7 defaults / task-14 fixtures 439 先例）。裁决：接受（同先例；扩场景先拆 scenario/ 子模块）。
- acquisition_gold 200 / failures 199 / view_gold 144 / main 4 纯行，全部 ✓。

**无 f64 / 确定性 serde**：两 src 文件通读，零浮点；见 Q1。

---

## 三、同提交登记检查（第 7 次强制：8 文件全 engine、零 docs/fixture）

代码头注 + notepad（issues §W3-Task 16 八条 + learnings §Task 16）携带的假设/决定清单，逐条裁决：

1. **dedup 语义 = 幂等 Ok（AlreadyAcquired）而非类型化拒绝**（notepad #5）：计划 Acceptance「一次获知只记一次」的幂等读法；双面锁定（outcome + 状态字节）。守卫顺序固定「属主→库存在+时刻≥公布→幂等」意味着重复 id 携带早于公布时点的 observed_at 会走 EarlyRead 拒绝而非幂等吸收——对物理不可能输入显式拒绝，符合铁律二。**接受**。
2. **`record_acquisition` 4+self 参数**（notepad #2，smell-2 豁免登记）：库引用是守卫协作者（属主→库→幂等三段校验必须原子于一个方法）；不引入一次性请求结构体。clippy 对该签名 0 警告。**接受**。
3. **QA failure 四项 = 三守卫 + 注入**（notepad #3）：见上节裁决。**接受**。
4. **AcquisitionError 无 PartialEq**（透传 InformationError 无该 trait，money.rs 禁改；task-19 先例）：断言用 `matches!` 字段绑定 + guard 精确比较——等价精确。**接受**（任务 27/29 若需相等性由 money owner 决定）。
5. **状态不预校验 id 存在于公开库（读取时透传库错误）**：acquisition.rs 头注明示；篡改态的库外 id 在 `context.report()` 透传 EarlyRead/UnknownPublication，不静默。**接受**。
6. **Market 泛型边界 + 观察触发/注意力接线待任务 25/26**：模块文档 + learnings 登记；本提交零提前接线。**接受**。
7. **discovery_candidates = 纯投影、无曝光权重模型**：K4「公共曝光仅改变发现机会」的候选面；权重模型属注意力系统（任务 25 session/attention.rs），计划任务 16 未要求。**接受**（正确最小范围）。
8. **中途自伤一次**（PS 管道 GBK 双重编码毁 acquisition_gold.rs，提交前整文件重写恢复；notepad #6 + learnings）：已验证提交树 acquisition_gold.rs 为干净 UTF-8（隔离 worktree 编译+运行通过即证）。**备案**。
9. **APPROVAL_HOUR=8 fold-in 裁定（任务指令点名）**：任务 15 复核 O-2 建议「任务 16/18/29 消费 approved_at 字段时并入 game-assumption-report-schedule 文案」。核查结果：任务 16 **产品代码不消费 approved_at**（acquisition.rs/npc_view.rs 零引用）；仅测试夹具构造 PublicationRequest 时必须填写该字段，用了同一已登记约定（publication.rs:27 `pub const APPROVAL_HOUR = 8`，任务 15 已裁定「不构成门禁违反」）。docs fold-in 未做——**裁定：非阻塞**，fold-in 触发点仍是首个产品消费者（任务 18/29）或 docs 清扫；夹具未引入任何新的未登记假设。次要：夹具两处硬编码 `8`（fixture.rs L206/L293）而非引用已导出的 `APPROVAL_HOUR` 常量（prehistory.rs L158 同场景用的是常量）——下次触碰时顺手统一，非缺陷（任意合法批准时点均可过 publication 守卫）。

**结论**：本提交仅依赖计划文本 + 已登记假设（notepad/learnings/代码头注），无未登记新假设 ⇒ 门禁通过。

---

## 四、worker 证据对账

| 证据 | worker 声称 | 隔离复跑 | 一致 |
|---|---|---|---|
| task-16-happy.txt | 13 passed / 0 failed | t16r-1：13 passed / 0 failed（同一测试二进制 information_acquisition-05607346d833d125） | ✓ |
| task-16-failure.txt | red = E0432 未解析导入 + E0614（实现不存在时的测试草案）→ green 7/7 failure 过滤面 | 逻辑自洽：E0432 证明 src 模块晚于测试；E0614 行号对应中间草案（最终文件用 matches! 引用绑定，无裸解引用） | ✓（TDD red 可信） |
| task-16-verify.txt | 13 passed / 0 failed | 同 t16r-1 | ✓ |
| 全量（无独立证据文件，learnings 声称 828/0/4 = 基线 815 + 13） | — | t16r-2：**828 passed / 0 failed / 4 ignored**，精确命中 | ✓ |

计数链条备注：task-15 learnings 自称 814/0/4（791+22=813 有 ±1 笔误嫌疑）；本任务基线 815+13=828 与隔离实跑精确一致，828−13=815 为 da6c399 真实基线。不影响本裁决。

计划复选框：`.omo/plans/...md` L407 任务 16 已被预标 `[x]`（zombie 先标，task-14 F-O7 同款流程观察）；本复核即为门禁，非阻断。

---

## 五、隔离执行记录（全部 exit code + 计数）

工作树：`C:\Users\msuad\AppData\Local\Temp\opencode\wt-review-16` @ 2d15e7c（detached）。主树零 cargo。

1. `cargo test -p engine --test information_acquisition` → **exit 0**；13 passed / 0 failed / 0 ignored（t16r-1.txt；2m14s 冷编译后 0.01s）。
2. `cargo test -p engine`（全量）→ **exit 0**；**828 passed / 0 failed / 4 ignored**（34 个套件结果行合计；含 information_acquisition 13、publications 22、extraction_replay 3/3、doc-tests 0）（t16r-2.txt）。
3. `cargo check -p engine -p server -p web-wasm -p engine-gpu` → **exit 0**；无 warning/error（t16r-3.txt；Finished dev profile 1m54s）。
4. `cargo clippy -p engine --all-targets` → **exit 0**；警告合计 7 条全部为既有登记项：behavior/decision.rs:239 filter_map（任务 1 起，指令明示不据此 REJECT）、tests/experience_feedback 3 条（任务 20 登记）、tests/analysis_profiles/invariants.rs 3 条 unusual_byte_groupings（任务 17 登记）。**任务 16 全部 8 文件 0 警告**（t16r-4.txt）。

---

## 六、非阻塞观察（供后继任务，不设独立任务）

1. **O-1（给任务 18/25/26）**：重复获知携**早于**公布时点的 observed_at 时走 EarlyRead 而非幂等吸收（守卫顺序使然，语义正确）——该组合路径无专属用例（两条 constituent 路径各有测试）；任务 25 接线注意力时若该顺序成为消费语义，补一条金样。
2. **O-2（给任务 18/25）**：多公司（≥2 公司、id 交错）的快乐路径无金样（现有金样单公司；跨公司仅篡改面）——首个多公司消费者顺手补。
3. **O-3（给任务 18/29）**：APPROVAL_HOUR=8 docs fold-in 债务仍开放（见三-9）；fixture.rs L206/L293 硬编码 8 → 改用导出常量。

## 七、结论

三个 AGENTS.md 问题全部通过；已登记偏差全部裁决接受；同提交登记检查通过（无未登记新假设）；隔离复跑与 worker 证据精确对账；K4 红线（公共曝光≠个人阅读、无前视、幂等获知、dev 分离、版本钉死、内存经济）逐条有测试锁定。**APPROVE**。
