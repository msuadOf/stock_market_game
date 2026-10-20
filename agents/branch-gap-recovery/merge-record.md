# 回收批合并执行记录（main ← fix/branch-gap-recovery）

> 合并执行 agent 产出台账。分支 tip `a5d0ba96`（4 提交，基 `a6911c52` 即 W3 合并点），
> 并入 main `fd5f210a`，合并提交 **`4975c69e`**（初始 `74006973` + 夹具适配 amend 重建，
> 见「amend 事故与恢复」）。分支内容台账（分页优化回收 + WASM 排查归档 + 虚报修正）由
> 回收批 agent 另行登记；本文件记录合并侧方式、双向校验、跨批适配与回归对照。

## 分支 4 提交清单（合并前独立核对）

| 提交 | 类型 | 内容 |
|---|---|---|
| `32f40c0a` | perf(engine) | `query_public_reports` 分页先截取（`.take(page_size + 1)` 免整页外摘要构建）；重立 3 例 has_next 边界特征测试；新增 `#[ignore]` 基准（4000 报告库，40 页漫游 799.8ms→42.4ms=18.9×，首页×20 838.4ms→19.6ms） |
| `22c2f57d` | docs(company-system) | 取回 WASM panic 排查台账归档（89 行，839e76b8 遗失物，内容零改动） |
| `99d0d446` | docs(company-system) | 修正 `current-handoff.md` 拆股「已接通全链路」虚报（S1 批代码未随 std-thread 重写存活） |
| `a5d0ba96` | docs(engine) | 更正分页基准夹具注释（直填改造后仅首条走 `from_parts`） |

合计 3 文件：`packages/engine/src/information/queries.rs`（+229）、
`agents/company-system/wasm-panic-investigation.md`（新增 89 行）、
`agents/company-system/current-handoff.md`（±6）。

## 合并方式

**普通 merge（'ort' 策略），文本零冲突。** merge-base `a6911c52`；main 侧领先 9 提交
（B4a 批，274 文件）。唯一同文件改动 `queries.rs`：main 侧 B4a 在 impl 头部新增
`query_report_availability`（+59），分支侧改 `query_public_reports` 方法体并追加尾部
`#[cfg(test)]` 模块——改动区域不重叠，自动合流干净。

**单父 commit-tree 重联未启用**（分支链虽为历史改写前旧链，但普通 merge 干净，
无需弃祖先）。

## 双向校验（B4a 同款方法）

- **A 侧**：合并结果 vs 合并前 main 的差异文件集 ≡ 分支侧 3 文件（+323/−2）✓
  （`diff <(git diff main@{1}..HEAD --name-only) <(git diff a6911c52..a5d0ba96 --name-only)` 零差异）
- **B 侧**：合并结果 vs 分支 tip 的差异文件集 ≡ main 侧 9 提交 274 文件（+38689/−760）✓
  （同法零差异）
- 即分支全量落地 + main 侧 B4a 全量保留。

## 跨批语义适配（合并暴露，非文本冲突）

分支分页基准/特征测试夹具 `report_with_id` 直填 `PublishedReport`，漏 B4a 披露域
扩展新增的 `source: PublicationSource` 字段 → lib test 编译红（E0063）。适配：

- `queries.rs` 测试模块 use 补 `PublicationSource`；夹具补
  `source: PublicationSource::SimulationAccounting`。
- **取值依据**：夹具报告集合由 `generate_report_set` 完整账务路径生成；生产前史
  播种路径（`company/groups.rs:311/352` 对 `SeededPrehistory` 报告走 `publish_closed`）
  固定产出 `SimulationAccounting`（`publish_simple_closed` 才是 `SimpleGenerated`）——
  夹具取值与生产语义精确一致。
- **与日界批撞车说明**：日界批 agent 已独立发现同一断裂并在其分支落
  `ab45c082`（同取值 `SimulationAccounting`，全限定路径写法）。本合并批独立等价
  修复先行完成并经全量回归验证，台账如实登记双批同修、语义一致，后续日界批合入时
  以先入 main 者为准自然消解。
- 该适配 5 行并入合并提交（B4a 合并「适配随合并提交」同模式）。

## 回归对照（合并态，128 核机，与日界批 agent 编译负载共存）

| 面 | 合并态结果 | main 基线（B4a 台账） | 判定 |
|---|---|---|---|
| engine lib 默认 | **2205 passed / 0 failed / 12 ignored**（271s） | 2202 / 0 / 11 | 恰 +3 passed（分支特征测试）+1 ignored（分支基准），零新失败 ✓ |
| engine tests 全量 | **3385 passed / 7 failed / 18 ignored（67 binaries）**；失败面仅 save_contract（33/5）+ session（136/2） | 3381 / 8（save_contract×5 + player_snapshot + full_day + desk r09 偶发） | 7 failed 全部在基线 8 集合内；desk r09 为偶发未触发；passed 较基线 +4 与偶发通过及负载相关波动方向一致（多通过非风险），如实登记；分支改动仅在 `#[cfg(test)]` 模块、tests binary 不含之，零新增失败 ✓ |
| 分页基准（--release --ignored） | full_walk 40 页 **52.1ms**；first_page×20 **23.3ms**；setup 4000 条 16.1ms | 分支侧优化后 42.4ms / 19.6ms（优化前 799.8 / 838.4） | 同量级（本机共存负载略高），优化保持、无回归 ✓ |
| wasm32 check | `cargo check -p web-wasm --target wasm32-unknown-unknown`（stable，RUSTFLAGS="" 覆盖）**通过**（4m16s） | B4a 通过 | ✓（nightly 工具链缺 wasm32-std 且 rustup 网络下载失败；ADR-0043 串行管线本就走 stable，stable 已装 wasm32 target） |
| host check | `cargo check -p web-wasm` 通过（32s） | B4a 通过 | ✓ |

注：本轮 tests 集成面 `cargo test --tests` 编译占 ~70 分钟（73 binary 全量链接，
rustc 满载并行），与日界批 agent 的 cargo 共存分核；运行阶段全部秒级。

## amend 事故与恢复（如实登记）

回归全绿后执行 `git add queries.rs && git commit --amend` 时，worktree 清理 agent
恰在同一 checkout 插入了其台账提交 `a341d9ba`（agents/worktree-cleanup/ledger.md），
amend 误打在**其**提交上（`0a51cd35`＝其内容+我的夹具修复，复用其 message）。
恢复：reflog 定位 → `reset --soft a341d9ba` → 修复 stash 摘出 → 在 `74006973` 上
重建含修复的合并提交（`4975c69e`，双亲 fd5f210a+a5d0ba96 核实无误）→
`cherry-pick a341d9ba` 原样重放清理提交（`50dc5dca`）→ main 归位。
内容级校验：diff-of-diff（分支原 queries.rs diff vs 合并 diff）差异仅 hunk 行号偏移
+import+source 适配行，分支改动逐字落地；模块复验 information::queries 3/0/1
ignored。教训：共享 checkout 上 amend 前必须核对 `HEAD` 是否仍为己方提交。

## 推送

`git push origin main`：`5a940264..50dc5dca`（memory 提交 fd5f210a + 合并 4975c69e
+ 清理台账 50dc5dca），origin/main = `50dc5dca` ✓。

## 独立复核

见下节（fork agent 复核结论）。

## 复核记录

fork 独立复核（未实施者，对 `git diff fd5f210a..4975c69e` 全量）结论：

- **大 A 语义 ✓**：链路 cursor 校验 → `reports_for_company`（BTreeSet 升序 +
  `published_at <= as_of` 过滤，先可见后截取）→ `id > cursor` → `take(page_size+1)`
  → map → `has_next = len > page_size` → truncate+游标；cursor 十进制语义未动。
  source=SimulationAccounting 与生产前史播种路径（groups.rs:311 → publish_closed）
  精确一致。
- **最小范围 ✓**：queries.rs=优化+注释+测试模块；适配仅 import+source+注释；
  直填夹具有明示理由且首条走生产校验，非夹带。
- **遗漏检查**：三特征测试真锁三边界；基准 #[ignore] 不入常规面；trading-rules
  无拆股节陈述属实。**发现 1 项（建议级高优先，已修复）**：current-handoff.md
  拆股勘误段「无 share_split、无 ShareReDenomination、无 ex_reference_price」三
  「无」按分支基线写作，B4a 合并后失实（纯域半三符号已在库）。
- **阻断项 0；建议项 1。**

建议项修复（随台账同批 docs 提交）：current-handoff.md L78 两处措辞修正——
「无任何实现」句改为如实登记「纯域半已随 B4a 在库（三符号列名）、session 接线
（approve_share_split）仍未落地」+ 标注 2026-10-20 合并复核修正来源；「由 B4a
在途回收」改为「已随 B4a 合并入 main」。复核另注（非本 diff，未动）：
`simple/preferences.rs:35` 注释称 `approve_share_split` 已落地，为 B4a 侧预存
失实，留 B4b/B4e 批顺手更正。
