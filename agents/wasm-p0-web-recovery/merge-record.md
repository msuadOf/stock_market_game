# wasm 恢复链两合并台账（P0 前置批 + W3 桥修复）

- 执行：wasm 链合并 agent，2026-10-20，主 checkout 独占
- 起点 main=bf7cc263（工作区干净），终点 31d47472，已推送
  origin/main（bf7cc263..31d47472）
- 分支台账来源：`.worktree/wasm-p0/agents/wasm-p0-web-recovery/record.md`、
  `.worktree/w3-bridge-fix/agents/w3-bridge-fix/record.md`（两分支各自随合并带入
  `agents/wasm-p0-web-recovery/record.md`、`agents/w3-bridge-fix/record.md`）

## 合并 1：feat/wasm-p0-web-recovery → 2bd74818

- 基 a6911c52（旧基）。预期冲突集中处（B4a 改 web 89 文件）经文件集比对
  **实际零重叠**：main 自基点改 278 文件中 apps/web 89 个（生成类型+协议域），
  与 P0 分支 16 文件无交集 → git 自动合并，无手工冲突。
- 双向校验 4/4 PASS：
  - vs main 差集 ≡ P0 分支文件集（16 文件，diff 输出一致）；
  - vs 分支差集 ≡ main 侧文件集（278 文件）；
  - 内容级：`diff(bf7cc263→HEAD)` 与 `diff(a6911c52→分支)` patch 全等；
    `diff(分支→HEAD)` 与 `diff(a6911c52→bf7cc263)` patch 全等
    （两侧文件集不相交时的强等价证明）。

## 合并 2：fix/w3-ingress-bridge-serial-crash → 732f5038

- 基 fd5f210a（新基）。main 侧仅改 5 文件（4 台账+engine queries.rs），
  与 W3 6 文件无文件级冲突；唯一双侧共改 `scripts/smoke-pages.mjs`
  （P0 加断言/日志字段 vs W3 去 stocks 裁剪，改动区域分离）git 自动并。
- 双向校验 PASS：vs 合并 1 差集 ≡ W3 文件集（6）；vs W3 差集 ≡ main 侧集；
  内容级：W3 侧 5 文件 blob 等于分支、main 侧 5 文件 blob 等于合并 1；
  smoke-pages.mjs 逐侧 diff 核对——vs P0 侧差恰为 W3 贡献（去裁剪+五股日志
  勘正），vs W3 侧差恰为 P0 贡献（wasmThreads/isolationHeadersAtOrigin 字段、
  隔离/SAB/标记三断言、worker 隔离断言、尾日志勘正）=两侧语义并集，
  无断言弱化、无回退。

## wasm-pkg 重建（web 测试前置）

- 主区原产物为 wasm-bindgen-rayon threads 错配构建（桥批备注），按官方
  `frontend-build-plan` wasm-pack 段重建：稳定工具链 1.96.1、串行
  `--target web --release`、CARGO_BUILD_JOBS=128；`check-web-release-wasm`
  （含 P0 新增 glue 契约断言）过检；产物无 rayon glue
  （grep startWorkers 0 命中）。备注：`/home/baiyifan/data1` 为指向
  `/data1/baiyifan/workplace` 的符号链接，wasm-pack 日志的前缀路径与主
  checkout 同一目录，已核实。

## 回归矩阵

| 项 | 结果 |
|---|---|
| scripts 触及面测试（pages-isolation/check-web-release-wasm/package-static-web/distribution-workflow） | 24/24 绿 |
| web 全批（194 文件/8 shard，run-web-tests.mjs） | **193/194 绿**；`mobile-color-contrast.test.ts` 环境性抖动，见下节"已知环境致因" |
| 集成 smoke-pages（真浏览器，dist-pages） | **全绿**（"Pages smoke passed including cleanup…"完整通过：双入口 bootstrap、隔离/SAB/标记三断言、五股会话入串行 WASM Worker、.app-root+无 app-error、worker 隔离断言）——P0+W3 全链首次端到端贯通 |
| tsc -b | 31 错=基线（company/report 域，与合并前文件清单逐文件一致；基线 31-32 不扩，合并/修复文件零错） |
| wasm check | 重建产物过检 + smoke 内置 verifyWebReleaseWasm(dist-pages) 过检 |
| engine lib（cargo test -p engine --lib） | **2205 passed / 0 failed / 12 ignored**（283.77s；首跑在负载 537 下 480s 超时被杀、无总结行=不算，负载回落后重跑全绿） |
| oxlint --deny-warnings（修复触及 4 文件） | 零告警 |

## 合并后发现的第 4 层断裂（既有缺陷，已修，commit e7e32aa5）

- 现象：smoke 首跑 `.app-root` 出现后 `.app-error` count=1，错误边界报
  `RangeError: 连续竞价 tick 1 尚未结束开盘窗口`（UI_RENDER_FAILED）。
- 归因：本地起局首快照 tick 必在开盘集合竞价窗口内，`LocalRefreshViews`
  两处以 `tick > 0` 守卫直调 `TradingTimeline.minuteSlot`，而该方法对
  开窗窗口 tick **按契约抛错**（既有测试
  `assert.throws(() => timeline.minuteSlot(10), /开盘窗口/` 明示设计）。
- 既有性证明：崩溃面三文件（trading-timeline.ts / LocalRefreshViews.tsx /
  market-chart-projection.ts）blob 在 main(bf7cc263)、合并 HEAD、W3 分支
  三处完全一致；且用 W3 分支自己的 dist-pages 复现同崩溃——W3 台账 smoke
  绿是断言与崩溃竞速获胜（其版本在 .app-error 检查后无复查），前层
  DataCloneError 一直在遮蔽本层。
- 修法（TDD）：`TradingTimeline.currentMinuteSlot(tick)`——开盘窗口内
  （含 tick 0）返回 null（"形成中分钟桶"不存在，MinuteKlinePanel 已按
  null 渲染"当前没有形成中的连续竞价桶"），`tick > openingTicks` 才换算；
  两调用点替换。红证（方法不存在）→绿；受影响视图测试 47 例全绿。
  纯展示层换算，文件头明示"仅换算展示坐标"，不涉交易语义。

## 已知环境致因（未修，如实登记）

- `mobile-color-contrast.test.ts` 批内超 10s：**裸 `browser.close()` 本机
  当前稳定 ~10.0s**（两次实测 10076/10062ms，与页面内容无关），加测试本体
  ~0.5s 即贴死 10s 案例硬上限；close 快时（~7s）单跑 7.98s 可过。该文件
  blob 在 main/W3/HEAD 一致、未被任何一侧改动；W3 worktree（=main 同输入）
  负载 64 下同样超时=main 基线当前同红，非合并回归。5 次批跑均 7/8 shard
  全绿+第 6 shard 其余 26 文件逐个验证绿。按十秒硬上限条款做了合规批内
  提速（commit 31d47472：34 次 evaluate 往返合 2 次，断言逐字保留，
  单跑 10.6s+→8.9-9.9s）；close 的机器级延迟非仓库可修，留待机器状态
  恢复或后续批处置（如 fixture 进一步缩小/浏览器复用需设计裁量）。
- 全程机器外部负载高（bazel/clangd/其他 agent 全仓测试构建，load 52-537）：
  web 批 shard 死亡与 engine 首跑超时均属墙钟挤占，均有纯净对照归因。

## 提交与推送

| 提交 | 内容 |
|---|---|
| 2bd74818 | Merge feat/wasm-p0-web-recovery |
| 732f5038 | Merge fix/w3-ingress-bridge-serial-crash |
| e7e32aa5 | fix(web) 开盘窗口 currentMinuteSlot 守卫（第 4 层断裂修复） |
| 31d47472 | test(web) 颜色对比度测试合批测量（10s 上限合规提速） |

- 已推送：`git push origin main`（bf7cc263..31d47472）。
- 复核：见下节。

## 独立复核（fork，未实施者）

结论：**A-F 全 PASS，可保持"已合入+已推送"，无有效发现需回滚或修复。**

- A（W3 桥两形态分派）PASS：lib.rs 649/663 两路同入
  `enqueue_player_intent(AccountId(0))`；SAB 探测 typeof 先行；generation
  校验两形态同强度（worker 侧 requireGeneration+host 侧响应后复查）。
- B（smoke 断言并集）PASS：三断言+worker 隔离断言+日志字段+去裁剪全在；
  `git log -S 'stocks.slice'` 证实裁剪归因（6ac55d83 引入、c59d001e 删除）。
- C（currentMinuteSlot）PASS：边界/零开窗局不回退/面板 null 渲染/全仓
  生产调用点核对（仅剩 market-chart-projection.ts:58，输入域为
  continuousPoints）均核。
- D（合批测量）PASS：断言/阈值 4.5/消息模板逐字一致、未命中显式 throw、
  无断言删除。
- E（双向校验抽查）PASS：16 文件/6 文件差集与分支集一致。
- F（大 A 语义/分层）PASS：四段 diff 零触碰 engine/server/web-wasm/desktop；
  展示换算域归属正确；startup-policy 门禁显式报错不静默降级。

低严重度备注（后续批，不阻塞，照登）：
1. `market-chart-projection.ts:58` 直调 minuteSlot 的安全性依赖
   "continuousPoints 只含连续竞价 tick"的隐式契约，无显式守卫——engine 侧
   未来变更若送入开窗 tick 会复现同类抛错，建议后续批补域测试或守卫。
2. `worker-host.ts` `capabilities.sharedMemory: true` 串行下失真——W3 台账
   已登记的后续批顺手项（真线程批收口能力面时定真值）。
