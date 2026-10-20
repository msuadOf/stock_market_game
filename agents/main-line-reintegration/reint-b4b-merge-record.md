# B4b 批合并执行记录（main ← feat/reintegration-b4b-session-assembly）

> 合并执行 agent 产出台账。分支 tip `7ec085a8`（6 提交，基 `fd5f210a`），并入
> main `58054ead` ⇒ merge commit `addfdc0f`（--no-ff），已推送
> （`58054ead..addfdc0f main -> main`）。实现批台账见同目录
> [`reint-b4b-record.md`](reint-b4b-record.md)；预存债批对照台账见
> [`../main-line-debts/main-line-debts-ledger.md`](../main-line-debts/main-line-debts-ledger.md)。

## 合并与冲突

- **git 冲突 0 个**：双侧共同触碰仅 2 文件（`session/persistence.rs`、
  `tests/session.rs`），且改动位于不同 hunk——分支侧（fault2101 在场性配对 /
  sample_setup 8 字段）与 main 侧（candle_calendar 归域 + 2090-2096 CompanyId
  数值化 / 玩家现金断言锚定）互不重叠，ort 策略自动合并。
- **双向校验双 PASS**：merge vs main 差异文件集 ≡ 分支自基点改动集（54 文件）；
  merge vs 分支 tip 差异文件集 ≡ main 侧自基点改动集（51 文件）。即 B4b 全量
  落地 + main 侧三批（回收/P0+桥/预存债）改动全量保留。
- 交叉文件内容级核对：merged vs main 的 persistence.rs 差 = 分支侧 fault2101
  单处；merged vs 分支的 persistence.rs 差含 main 侧归域
  （`map_err(CivilClockError::from)` 两处逐字在树）——**预存债错误域归位与
  CompanyId 数值权威零回退**。

## 交叉点语义裁决（双侧同域但非文本冲突）

### tampered_calendar_policy（B4b × 预存债债 2）

两侧各自处置过同一测试，方向不同：

- **B4b**（分支 `failures.rs`）：把断言改「或」形态——decode 层
  `SessionError::Calendar(DigestMismatch)` | restore 层
  `SessionError::CivilClock(CivilClockError::Calendar(DigestMismatch))` 双信封，
  承载同一类型化载荷（field/stored/computed 全保留）。
- **预存债批**（main `persistence.rs`）：判定 decode 层 From 直落 `Calendar` 域
  为**真 bug**（同篡改两校验点错误域不一致），修代码归域
  `CivilClock(CivilClockError::Calendar(..))`，测试零改动。

**裁决=零适配自然并集**：合并树代码保留归域（main 侧），测试取 B4b 或形态
（两臂其一命中即过）——实际行为以 `CivilClock` 臂命中，回归 save_contract
**38/0** 证实。断言强度未弱化（双信封类型化断言仍在，只是兼容归域后的域）。

### 其余交叉

无。save 面旧档字节零变化（Option + `skip_serializing_if`）不受 main 侧
save_contract 收口影响（后者只动错误域与 ctx 数值，不动序列化形态）；web
company-ids tsc 架构（main 侧）与 B4b `SessionSetup.ts` 生成物（+51 行）无
共享符号，tsc 0 错证实。

## 回归对照（合并树 addfdc0f，2026-10-20）

| 面 | main 基线 | 合并树 | 判定 |
|---|---|---|---|
| engine lib | 2205/0 | **2206/0**（+1 容器 serde 契约）| 绿 |
| engine tests 面 | 3391/1 | **3408/1**（+16 验收 +1 lib；67 integration binaries 1202/1 + lib 2206/0）| 红集不增：唯一红 `full_day_accounting_rejects_a_missing_day_boundary` = main 双亲预存，desk r09 未触发 |
| save_contract | 38/0 | **38/0** | 预存债收口零回退 |
| 新测试 session_company_foundation | —（分支 16/16）| **16/16** | 复跑绿 |
| web tsc | 0 | **0** | 31 错清零保持 |
| web 单测（run-web-tests.mjs）| 全过 | **897/897 fail 0**（7 suite）| 绿 |
| wasm32 check（engine / web-wasm）| 绿 | **双包 Finished**（RUSTFLAGS=""）| 绿 |
| B 探针（详见下）| ±2% 门 | **中位 -0.02%** | 门内 |

执行方式登记：tests 面 68 binaries `xargs -P 32` × `--test-threads 4` 分片并行
（规约多核要求）；engine lib binary 在 32 路争核下一次 300s 超时（124），单独
复跑 2206/0（227.9s）——如实登记，非测试本体问题。

## B 探针对照（driver_semantics_probe，3000 tick 主口径）

- 臂：P=pristine main `58054ead`（worktree `.tmp/b4b-probe-main`，binary sha256
  前 16 `e2d8a7a9b288aa23`）；M=merge `addfdc0f`（`e254e7ab37ee89ea`）。
- 跑法：`taskset -c 96-111` 钉核，warm 后交错 5 对，
  `probe .tmp/driver-semantics-setup.json 3000 16`（5 股/16 runtime_groups，
  setup 同 B4b 批）。
- wall_s 逐对比值：-0.54% / +0.23% / -1.01% / +0.37% / -0.02% ⇒
  **中位 -0.02%，均值 -0.19%**——±2% 门内（与 B4b 分支验收时 +0.02% 同量级）。
- 守恒与事件数：两臂 cash_initial==cash_final、shares_ok=true、events=21917
  逐位一致。

## 独立复核（fork，未实施合并方）

**复核通过，无 P0/P1**（2026-10-20，只读复核 `58054ead..addfdc0f`）：

1. **预存债收口未回退 PASS**：合并 diff 对两共享文件零删除行（纯新增），
   回退物理不可能；55a9d6df 的 15 个 web 文件与 2a4db2bc 的
   bank_policy.rs/restore_guards.rs 均不在合并 diff（唯一 web 文件是
   SessionSetup.ts 生成物）。锚点逐点在树验证存在：
   `map_err(CivilClockError::from)`×2、`u64::from(id.0)`×7（fault2090-2096）、
   proposed_defaults 同源锚定、company-ids.ts、restore_guards fault2080 码断言。
2. **无越界 B4c/d PASS**：corporate_actions.rs 公开面仅 configure_registry/
   configure_cash_dividend_tax_book/taxpayer_identity_of_kind + 私有
   validate_registry；全树无 close_registries_through、无 AdminRight、无
   process_*_day_end、无 ts(export) 翻转（新面均 ts(skip)，符合 B4f 过渡声明）；
   diff 中 B4c/B4d 命中全部为注释边界声明。
3. **语义并集+大 A PASS**：新字段 serde 形态完备（default + skip_serializing_if），
   旧档字节零变化由逐 key 断言钉住；fault2101 位图配对/fault1048 残缺位图/
   三态链成立；tampered_calendar 或形态断言与归域后 CivilClock 域兼容，
   DigestMismatch 类型化载荷断言强度保留。

- **P2×1 登记（非本合并义务，留后续批收口）**：failures.rs tampered_calendar
  断言的 `SessionError::Calendar` 臂在预存债归域后不可达（死臂）——该 bug 若
  复发此测试不设防；现兜底为 session_boundary.rs 的 CivilClock 域锚测试。
  建议后续批把断言收口为单臂 CivilClock 形态。
