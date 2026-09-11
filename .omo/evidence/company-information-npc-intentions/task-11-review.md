# Task 11 independent review — 地产开发、预售与交付会计 (commit cad7ac7)

Reviewer: independent subagent per AGENTS.md 大A语义与独立复核门禁 (did NOT implement).
Scope reviewed: full diff of `cad7ac7` (`feat(engine): 支持地产预售交付与项目会计`, parent
`4e830c5`) — 24 files, +3544/−3. All execution done in isolated worktree
`C:\Users\msuad\AppData\Local\Temp\opencode\wt-review-11` (main tree has a concurrent
insurance agent's uncommitted work; no cargo was run there).

VERDICT: APPROVE (converted after remediation 070ee58; original rejection preserved in §5/§9-history)

Original rejection reason (at cad7ac7, now resolved — see §9): the engineering is verified
sound, but the commit implements 借款费用资本化 past a **task-specific, test-locked
registration gate** (fixture row
`cas-17-borrowing-costs-2006`: 「地产开发借款费用资本化（K3）在补证前不得实现」 /
「任务 11 实现地产资本化前必须先取得原文」; docs/company-accounting.md §2.5 row ⛔
「任务 11 实现的先决条件」) **without converting that registration to the precedented
game-assumption state** — the assumption ID `game-assumption-borrowing-capitalization`
that the code relies on exists nowhere in `policy-sources.json` or
`docs/company-accounting.md`. The canonical, machine-readable policy registration now
contradicts the code (cross-layer semantic drift, gate question 3). Remediation is a small
docs+fixture addendum commit; no code change is required. Details in §5.

---

## 1. Isolated execution record

| # | Command (worktree wt-review-11 @ cad7ac7) | Exit | Result |
|---|---|---|---|
| 0 | `git worktree add C:\Users\msuad\AppData\Local\Temp\opencode\wt-review-11 cad7ac7` | 0 | worktree created (output t11r-worktree.txt) |
| 1 | `cargo test -p engine --test real_estate_accounting` | 0 | **20 passed / 0 failed / 0 ignored** (t11r-1.txt), 0.01s |
| 2 | `cargo test -p engine` (full package, clean tree) | 0 | **711 passed / 0 failed / 4 ignored** across 28 result lines (t11r-2.txt) |
| 3 | `cargo check -p engine -p server -p web-wasm -p engine-gpu` | 0 | clean (t11r-3.txt) |
| 4 | `cargo clippy -p engine --all-targets` | 0 | warnings only (t11r-4.txt), see adjudication below |

Cross-check against worker evidence:
- task-11-happy.txt claims 20/20 → **verified** (isolated 20/20, same test names).
- task-11-failure.txt = 19 passed / 1 filtered out (gold filtered) → consistent subset of
  the same 20.
- provenance.txt claims full-package 711/0/4 → **verified identical** on clean tree;
  `extraction_replay::identical_construction_replays_bit_identical` is green at cad7ac7,
  confirming the worker's mid-work blockage was solely the concurrent task-20 agent's
  then-uncommitted diff (honestly recorded in provenance.txt + issues.md item 5).
- Worker claimed "27 suite targets"; actual clean-tree count is 28 `test result` lines
  (incl. doc-tests). Trivial counting discrepancy, no impact.

Clippy adjudication (all warnings located, none in task-11 files):
- `behavior/decision.rs:239` `.filter_map`→`.map` — pre-existing (task-42 register);
  explicitly out of review scope.
- `tests/analysis_profiles/invariants.rs` ×3 (hex digit grouping) — pre-existing, not
  touched by cad7ac7.
- `tests/experience_feedback/main.rs` ×3 (too_many_arguments ×2, bool assert) — sibling
  task-20 agent's test, present in parent `4e830c5`, untouched by cad7ac7.
- **Zero warnings** in `company/real_estate/**`, `reports/real_estate.rs`, `journal.rs`,
  `tests/real_estate_accounting/**`. Worker's claim verified.

## 2. AGENTS.md gate questions

**Q1 — 是否符合大 A 语义，依据是否可靠？**
Presale/delivery chain: YES with reliable verified basis — 预售收款 Dr 现金/Cr 2203
合同负债 (presales.rs L116-179, CAS 14(2017) §39, verified-official per
docs/company-accounting.md §2.5 with MOF links); 收入只在交付=控制权转移时点确认
§4/§13 (delivery.rs, single entry Dr 2203/1122/6401, Cr 6001/1541); 尾款回收只清应收
(collect_final, revenue untouched — gold L191-201 asserts 6001 stays 30,000 元).
Capitalization: the implemented semantics are internally coherent and honest (see §5)
but their canonical-provenance registration is incomplete → this is the REJECT driver.
Cash-flow classification: 购地/开发/预售收款/尾款 = Operating (开发存货 = 开发商品的原
材料存货, CAS 31 residual-classification choice, documented in mod.rs L18-20 + land.rs +
learnings), 借款/付息/还本 = Financing — consistent A 股 treatment, registered.
VAT explicitly not modeled and registered (issues.md item 4). Units/labels consistent
across code/types/tests/docs (元↔分 convention declared in test main.rs L10-12).

**Q2 — 改动是否为需求所必需、是否保持最小范围？**
YES. Every handler maps to a K3 plan row (购地/开发/借款资本化/预售/交付/尾款/减值/
还本付息). The reports layer is classification-only as specified (task 13 consumes).
`journal.rs` +16 is additive enum variants with doc-comments only, appended at enum end
(save-compatible; extraction_replay byte-anchor green on clean tree proves SaveSlot
unaffected). `company/mod.rs` +6 pure re-export wiring. No speculative features; no
session/market coupling (`grep crate::session|crate::market` in real_estate/** → no
matches; credit/capitalization decisions read no market state). File-count deviation
(13 vs plan 8) adjudicated in §4 item 1 — justified.

**Q3 — 是否存在遗漏的边界测试、跨层语义漂移或不必要复杂度？**
Boundary tests: none missing — full battery present and passing (see §3).
Cross-layer semantic drift: **YES — the REJECT finding (§5)**: fixture/doc policy
registration vs implemented code.
Unnecessary complexity: none found — rhe_div twin copies are visibility-constrained
(6th copy, registered, task-13 consolidation noted); double remainder chains are
conservation-required by design.

## 3. K3 red-line audit (review probes, all verified in code + tests)

| Probe | Verdict | Evidence |
|---|---|---|
| 预售收款 = 合同负债 NOT 收入 | PASS | presales.rs L116-179 (Dr 1002/Cr 2203, `BusinessKind::PresaleCollection`); gold L114-119 asserts 6001 == 0 after collection; failure `CollectionAfterDelivery` closes post-delivery channel |
| 交付时冲合同负债、确认收入、结转成本，一次性 | PASS | delivery.rs L37-133 single atomic entry; gold L177-188: liability cleared 18,000 元, revenue 30,000 元 exactly once, COGS 1,212,720 分, 1541 → 808,480; `PresaleAlreadyDelivered` + `DeliveryBeforeCompletion` typed rejections |
| 未达到交付条件确认收入拒绝 | PASS | `DeliveryBeforeCompletion` (delivery.rs L58-63) + lifecycle.rs `rejects_delivery_before_completion` with byte-identical state assertion |
| 重复交付拒绝 | PASS | lifecycle.rs `rejects_duplicate_delivery` |
| 尾款回收不重复收入 | PASS | collect_final Dr 1002/Cr 1122 only; gold asserts revenue unchanged after tail collection |
| 无限资本化拒绝 | PASS | `day_capitalizable` ceased-check is permanent (projects.rs L212-221); capitalization.rs `post_completion_interest_must_expense_not_capitalize_forever` asserts 1541 byte-frozen across two post-completion accruals |
| 中断暂停资本化（开放+闭合两态） | PASS | projects.rs L224-235; gold accrue-2 (open interruption 91d ≥ 90 → all expensed), accrue-3 (closed 106d gap → 15d expensed / 16d after resume capitalized); short-interruption (30d < 90) keeps capitalizing |
| 项目借款偿付 = 筹资现金流 | PASS | debt_service.rs both handlers `CashFlowClass::Financing` |
| 开发成本归集到项目存货 | PASS | land.rs + development.rs Dr 1541; `development_inventory_total()` 勾稽锚 == 1541 balance asserted in gold L244-248 |
| 存货减值 | PASS | impairment.rs targeted top-up/reversal via explicit NRV; gold 108,480 分 contra + loss |
| 资本化窗口数学（手算复核） | PASS | see §4 item 2 / §6 — every cent reproduced |
| 超项目数量拒绝 | PASS | `ProjectCountLimit` + guards.rs test (max 2 → 3rd rejected) |
| 无现金支付拒绝 | PASS | `PaymentFailed` mapped from `BatchAborted{NegativeCashProhibited}` (error.rs L190-197); two tests (dev spend, interest payment), both assert full-state byte-identity |
| 拒绝不消耗事件 id / 状态字节不变 | PASS | `post_with_commit` advances `next_event_id` only after successful post (mod.rs L177-187); every failure test asserts `assert_eq!(re, before)` which includes `next_event_id` |
| Chart 版本无冲突 | PASS | `AccountChart::new(5)` (real estate v5); industrial v2 / bank v3 / v4 free for in-flight insurance — no collision |
| codes 单一真源 / 依赖方向 | PASS | `accounting::reports::real_estate::codes` is the single source; `company::real_estate::chart` re-exports it; accounting does not import company |
| reports 分类层只读 | PASS | real_estate.rs (87 行) reads net-debit balances only; no statement generation/close/cash |
| mod.rs 268 总行 vs 250 纯行天花板 | PASS | 199 pure lines (comments/blanks stripped) — under ceiling |
| gold.rs 312 总行 / 238 纯行 | PASS | single-scenario chain gold; registered in issues.md item 1 |
| 不要求默认地产股 | PASS | diff touches no defaults/session/market files; `CompanyKind::RealEstate` pre-existed in parent (spec.rs L37, planted with a task-11 pointer by an earlier task) |
| 任务 21 教训：成功路径全覆盖 | PASS | all 16 public surfaces exercised in the gold chain; state-machine guards reject only true violations (suspend→resume→complete and direct-complete both reachable) |
| serde 往返 | PASS | gold L308-311 byte-identical roundtrip |

## 4. Registered-deviation adjudication (issues.md items 1–7, learnings §584-632)

1. **13 files vs plan 8** (added chart/config/error/loans/debt_service): APPROVED.
   chart/config/error follow the bank/industrial assembly pattern (reviewed task 9
   precedent); `loans` is the state/ACT-365F-math twin (industrial/bank precedent);
   `debt_service` split exists because merged borrowing_costs would be 332 pure > 250
   (post-split 233/107 — verified by my count). All 21 task-11 files ≤ 238 pure lines
   (max gold.rs 238; worker claimed 237/232 — ±1 counting-method noise).
2. **资本化 = 版本化游戏假设, fixture 条目维持 blocked 不动**: **REJECTED — the core
   finding, see §5.** The labeling honesty itself is approved (see §5a); the registration
   placement is not.
3. **rhe_div 孪生副本（第六份）**: APPROVED — original is `pub(in crate::accounting)`,
   invisible to company domain; `InventoryLedger` cannot express add-cost-without-quantity;
   task-13 consolidation registered.
4. **已登记简化**（增值税未建模 / 无预售许可进度 / due_on=交付日 / 减值不随交付自动
   转回 / 单科目 1541 / 购地与开发记 Operating）: APPROVED — each outside the K3 plan
   row's requirements, each labeled at the code site and registered.
5. **并发 task-20 阻断 → 21 套件显式枚举等效证据**: APPROVED — clean-tree run now shows
   full 711/0/4 green including extraction_replay; worker's mid-work record is accurate
   and honest.
6. **fmt 在 HEAD 3bb5096 对 bank/industrial/defaults 本就不绿**: NOT RE-RUN by this
   review (fmt out of probe scope); claim consistent with observed clippy cleanliness of
   task-11 files. Non-blocking.
7. **BusinessKind +7 additive variants**: APPROVED — appended at enum end, doc-comments
   only, no posting logic; save-compat proven by green extraction_replay at cad7ac7.

Unregistered deviations hunted: none found beyond the above (notepad entries 1–7 cover
the diff faithfully; commit timestamp 03:46 before provenance.txt 03:48 is a benign
post-commit evidence write, recorded transparently inside provenance.txt itself).

## 5. CRITICAL provenance adjudication — CAS 17 (the REJECT finding)

**(a) Honesty of code labels: PASS.** No CAS 17 clause number is cited anywhere in
task-11 files (grep for 第十七条/§N near CAS 17 → only a pointer to
docs/company-accounting.md §7, the blocked-list section, which is a cross-reference not
an authority claim). Every touchpoint labels the policy as 版本化游戏假设
`game-assumption-borrowing-capitalization` and disclaims CAS 17 compliance: mod.rs
L14-17, projects.rs module doc L9-17, config.rs L5-7, journal.rs variant doc, test
main.rs L5-12. Suspension threshold 90d is Fixture-marked. CAS 14 §39/§4/§13 citations
(presales/delivery) ARE backed by verified-official rows — legitimate. impairment.rs
labels NRV as Fixture game assumption with CAS 8 原文取证受阻 noted. The worker also
performed and recorded the mandated extra retrieval round (tfs.mof.gov.cn 令/档 channel,
three URLs, all 「页面不存在或已删除」/index-only-modern-部令) in provenance.txt L3-16.

**(b) Registration: FAIL — this is the rejection.** The repo's canonical registration
pair is `docs/company-accounting.md` + `policy-sources.json` (task 2 established both;
fixture is machine-readable and locked by `policy_manifest`). At cad7ac7:
- fixture `cas-17-borrowing-costs-2006` (unchanged) still reads:
  `blocked_reason: 「…地产开发借款费用资本化（K3）在补证前不得实现」`,
  `summary: 「任务 11 实现地产资本化前必须先取得原文」`; the real_estate coverage row
  `符合条件借款费用资本化 | ["cas-17-borrowing-costs-2006"] | blocked`.
- docs §2.5 row still reads `⛔（任务 11 实现的先决条件）`; §7 解除条件 still reads
  `任务 11 开工前`.
- The assumption ID `game-assumption-borrowing-capitalization` appears in **no** docs or
  fixture artifact — only in code comments, notepad issues.md/learnings.md, and the
  evidence provenance file.

So the test-locked machine-readable policy layer asserts that this commit's central
feature "must not be implemented", while the code implements it. A future agent reading
the declared source of truth would conclude the feature does not exist / must not exist.
That is cross-layer semantic drift on a profit-timing-sensitive rule (capitalizing
interest defers expense — adjacent to the K3 不无限资本化 red line), i.e. exactly what
this review gate exists to catch, and per the charter 有效发现必须修复并再次复核.

Precedent analysis (why this is rejection-grade despite the notepad registration):
- The STRONG precedent (task 8, inventory pricing) registered its game assumption in
  BOTH artifacts: fixture entry `game-assumption-inventory-method`
  (status `simulated-game-assumption`) + coverage row source_ids + doc §2.1 row 🎮.
  Task 11 skipped all three despite relying on the same mechanism.
- The cas-17 gate is task-specific: task 2 wrote this row explicitly as task 11's
  precondition (「任务 11 实现的先决条件」/「任务 11 开工前」). Blowing past your own
  registered, targeted gate without converting the registration is categorically worse
  than the weaker precedent below.
- The WEAK precedent (task 8 industrial fixed-asset impairment, capex.rs L99-123,
  implemented while the generic cas-8 row stays blocked) shows a looser pattern already
  in approved history — recorded here as a non-blocking consistency observation (§7),
  not as license: cas-8's fixture text is generic (「任何…实现前须补原文」) and was not
  planted as a named task's precondition.

**Required remediation (small docs+fixture addendum commit; no code change):**
1. `policy-sources.json`: add assumptions entry `game-assumption-borrowing-capitalization`
   (kind `game-assumption`, status `simulated-game-assumption`), recording the window
   semantics (首笔开发投入起 / 完工永久终止 / 闭合中断 ≥ suspension_min_days 暂停 /
   开放中断计提时判定 / 不追溯重述), parameter versioning, and all three retrieval
   rounds (two kjs rounds inherited + this task's tfs round with URLs and outcomes);
   append the tfs attempts to `cas-17-borrowing-costs-2006.attempts`; update the
   real_estate coverage row `符合条件借款费用资本化` source_ids to
   `["cas-17-borrowing-costs-2006", "game-assumption-borrowing-capitalization"]`.
2. `docs/company-accounting.md` §2.5: convert the capitalization row to 🎮 referencing
   the assumption ID with the blocked cas-17 source kept as provenance record; update
   §7's cas-17 解除条件 to reflect the assumption path (text retrieval now upgrades the
   assumption rather than gates the feature).
3. Re-run `cargo test -p engine --test policy_manifest` + full engine suite (must stay
   711+/0/4 shape) and submit the addendum diff for a focused re-review of this finding.

## 6. Hand-verified gold-chain math (chain_gold…, gold.rs)

Rate construction: 1,000,000 分 @ 730bp ⇒ per-day interest = 1,000,000×730/3,650,000 =
exactly 200 分, zero remainder (deliberate fixture). Day windows (all [from, through)):
- Accrue 1 [1/1, 3/31): 31+28+30 = 89d, window open (dev started 1/1, no interruption,
  completed=None) → 89 cap = 17,800; 1541 = 1,200,000+800,000+17,800 = 2,017,800 ✓
- Accrue 2 [3/31, 6/30): 91d; open interruption since 3/31, elapsed 6/30−3/31 = 91 ≥ 90
  → all 91 expensed = 18,200; 1541 byte-frozen ✓ (开放中断规则 L230-233)
- Resume 7/15 closes [3/31, 7/15) = 106d ≥ 90. Accrue 3 [6/30, 7/31): 31d = 15d inside
  closed long gap (expensed 3,000) + 16d after 7/15 (cap 3,200) ✓
- Complete 8/1. Accrue 4 [7/31, 8/1): 7/31 < completed 8/1 → 1d cap = 200; 1541 =
  2,021,200 ✓ (边界日判定 L218-220, day >= completed 才终止 — 7/31 仍资本化正确)
- Accrue 5 [8/1, 9/1): 31d all ≥ 8/1 → expensed 6,200; FIN_EXP total 27,400; 1541 frozen
  at 808,480 post-delivery ✓
- Conservation: cap 21,200 + exp 27,400 = 48,600 = 243d × 200 (89+91+31+1+31 = 243 ✓);
  carried_cap = carried_exp = 0, accrued_unpaid = 0 after payment ✓
- Carry-out: rhe(2,021,200 × 6/10) = 1,212,720 exact (no rounding residue); remaining
  808,480; Σcarried + remaining == land + dev + cap-interest asserted ✓
- Impairment: gross 808,480 − NRV 700,000 → 108,480 top-up ✓
- Cash: 3,000,000 +1,000,000 −1,200,000 −800,000 +1,800,000 +1,200,000 −48,600
  −1,000,000 = 3,951,400 ✓; NI = 3,000,000 −1,212,720 −27,400 −108,480 = 1,651,400 ✓;
  A = 3,951,400 + 808,480 − 108,480 = 4,651,400 = L(0) + E(3,000,000 + 1,651,400) ✓
- Presentation lines mirror all of the above ✓; serde roundtrip byte-identical ✓

Non-restatement semantics cross-checked across layers (projects.rs doc L16-17,
`day_suspended` implementation, gold accrues 2/3, issues.md item 2, learnings L601-603):
each day classified exactly once at the accrual covering it, using then-current
interruption knowledge; no day is double-counted and no retrospective entries exist —
deterministic and consistently declared. Acceptable as a registered game assumption.

## 7. Non-blocking observations (for the orchestrator; no action required for this task)

1. **cas-8/impairment parallel**: industrial (task 8) and now real-estate implement
   impairment while the cas-8 coverage rows stay blocked with no game-assumption entries
   (NRV-目标化 semantics are labeled Fixture in code). Recommend one provenance-consistency
   pass (could ride along with the §5 remediation's reviewer) deciding uniformly whether
   blocked-but-implemented events get game-assumption entries. Not rejection-grade here:
   generic gate text, weaker/older precedent, honest in-code labeling.
2. Worker's suite-target count (27 vs 28) and pure-LOC ±1 vs my counts — cosmetic.
3. `accrue_interest`'s `UnknownProject` arm for loan-linked projects is defensive-only
   (projects are never removed after borrow-time validation) — harmless, no dead-end.

## 8. What this REJECT does and does not contest

Contested: canonical registration of the capitalization game assumption (§5) — fix is
docs+fixture only. NOT contested (all verified passing in isolation): the accounting
semantics, the K3 red lines, the capitalization window math, remainder-chain
conservation, the failure battery, save compatibility, chart versioning, dependency
direction, LOC ceilings, absence of default real-estate stock, and the worker's evidence
integrity (every recorded number reproduced: 20/20, 711/0/4, clean check/clippy-on-new-
files). After the §5 addendum lands and is re-checked (policy_manifest + full engine
suite green), this review converts to APPROVE with no further code conditions.

## 9. Re-verification after fix 070ee58 (2026-09-11)

Remediation commit `070ee58` (`docs(engine): 登记地产借款费用资本化游戏假设`, parent
`cad7ac7`). Isolated execution in fresh worktree
`C:\Users\msuad\AppData\Local\Temp\opencode\wt-review-11b` @ 070ee58 (main tree still
carries the concurrent insurance agent's uncommitted work; no cargo run there, no files
modified there except this review file).

### 9.1 Diff check vs §5 spec — all four elements present, no deviation

`git show --stat 070ee58`: **exactly 2 files** — `docs/company-accounting.md` (+2/−2)
and `packages/engine/tests/fixtures/company-model/policy-sources.json` (+20/−4).
**No .rs file changed.**

- **(a) Fixture assumption entry** ✓ — new `game-assumption-borrowing-capitalization`
  appended to the assumptions array after `game-assumption-inventory-method`:
  `kind: "game-assumption"`, `status: "simulated-game-assumption"`,
  `game_assumptions[5]` carrying the full window semantics (begin at 首笔开发投入 under
  the three-condition phrasing / suspend at Fixture `suspension_min_days` default 90 with
  open-interruption-at-accrual and closed-interruption-at-closure rules / permanent cease
  at 实质完成/可使用 with out-of-window expensing / versioned parameters / 不追溯重述)
  plus the explicit 「不构成 CAS 17 合规声明」 disclaimer.
- **(b) Coverage row** ✓ — real_estate `符合条件借款费用资本化` source_ids now
  `["cas-17-borrowing-costs-2006", "game-assumption-borrowing-capitalization"]`; row
  status remains `"blocked"` — identical shape to the inventory-method precedent row
  (blocked source + registered assumption), consistent.
- **(c) cas-17 blocked row amended** ✓ — 「不得实现」/「必须先取得原文」 language
  removed; blocked_reason now 「原文仍未取得（见 attempts）；地产开发借款费用资本化现按
  已登记的游戏假设运行」; summary carries the unblock-and-reanchor path 「取得 CAS 17
  原文后，为覆盖行重新锚定官方依据」; third retrieval attempt appended with all three
  tfs.mof.gov.cn URLs, outcomes, and date (2026-09-11).
- **(d) Docs** ✓ — §2.5 capitalization row ⛔→🎮 citing the assumption ID with the rule
  text and 「不构成 CAS 17 合规；CAS 17 原文仍未取得，保留为 provenance」; §7 cas-17
  解除条件 rewritten to the reanchor path (no more 「任务 11 开工前」).

**Code↔doc semantic match re-checked** against `projects.rs`/`borrowing_costs.rs`:
the doc's begin clause is anchored by the 「（首笔开发投入起）」 parenthetical to exactly
what `dev_started_on` implements; suspend/cease/non-restatement clauses match
`day_suspended`/`day_capitalizable` and the accrual-time classification verbatim. No
semantic drift.

Cosmetic nit (non-blocking): the new fixture entry's `retrieval_date` is `2026-09-10`
while its third tfs attempt is dated 2026-09-11 in `attempts` — the attempts array
carries the true dates, registration date one day earlier; no material effect.

### 9.2 Isolated re-run results at 070ee58

| # | Command (worktree wt-review-11b @ 070ee58) | Exit | Result |
|---|---|---|---|
| 0 | `git worktree add …wt-review-11b 070ee58` | 0 | created (t11f-worktree.txt) |
| 1 | `cargo test -p engine --test policy_manifest` | 0 | **5 passed / 0 failed / 0 ignored** (t11f-1.txt) — incl. `manifest_fixture_is_valid` green, confirming the new entry passes the locked structural schema |
| 2 | `cargo test -p engine --test real_estate_accounting` | 0 | **20 passed / 0 failed / 0 ignored** (t11f-2.txt) |
| 3 | `cargo test -p engine` (full, clean tree) | 0 | **711 passed / 0 failed / 4 ignored** across 28 result lines (t11f-3.txt); `extraction_replay::identical_construction_replays_bit_identical` green — byte-identical to the cad7ac7 counts, as expected for a docs+fixture-only addendum |
| 4 | `cargo check -p engine -p server -p web-wasm -p engine-gpu` | 0 | clean (t11f-4.txt) |

Fix worker's claims (policy_manifest 5/5; real_estate 20/20; full 711/0/4) — all
reproduced exactly.

### 9.3 Final verdict

The §5 finding is resolved in full: the canonical, test-locked registration now carries
the game assumption the code relies on, the contradicted prohibition text is gone, the
retrieval provenance (three rounds) is in the fixture, and the unblock path is recorded
in both artifacts. Everything contested in §8 was already green and remains green at the
remediation commit with zero code drift. **VERDICT: APPROVE** — task 11
(cad7ac7 + remediation 070ee58) passes the AGENTS.md independent-review gate with no
further conditions. Non-blocking §7 observations (cas-8 parallel, cosmetic nits) remain
recorded for the orchestrator.
