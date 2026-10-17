# 四审专项批交接（第 16 任 → 第 17 任，2026-10-18）

HEAD=`e825f0da`（前驱 ef9cc217/8d77d106）。工作区仅登记的 SaveSlot.ts。
上下文水位：第 16 任已 735k+，按协调者纪律停手写交接。

## 已落地（本批两提交）

1. **ef9cc217 尾巴批一**：fixture 股对定稿（000001 深主板+300001 创业板
   ——id0=1000 主板/id1=2000 创业板，语义与原 fixture 完全还原，A4 排序
   成立）；p2_3a golden 重锚；p2_4b 连续窗；restore 重复日结
   （WorldSpec.initial_last_day_end_tick）；统一归档点（日结拍 install
   不 push——A2PUSH/A2COMMIT 打点实证）；无成交日占位 K；净预置史基准
   （构造期长度−session.day）；**bridge.rs:94 配置透传**（A4 判决点——
   默认 GameConfig 覆盖已撤）。
2. **e825f0da 专项批二**：**cage 拒收回执拆分**（ReceiptKind::
   PriceCageExceeded 新变体+desk 双 kind 拆分+event_bridge 映射——
   RejectionReason::PriceCageExceeded 的 runtime 等价，session cage
   断言面收口）；**restore 镜像挂单回填**（mirror 从档回填在场委托/
   next_serial/cutoff——sparse FIFO 族"restore 静点 save 丢挂单"首因
   消除；desk 权威仍首拍 install 重建）。

## 当前数字（如实）

- lib 默认 **1650/4**（closing/p2_4b/sol7th/public_step——判型后置）
- lib ENGINE_STEP_RUNTIME=0 **1652/2**（p2_4b/sol7th——两模式均红=
  fixture/深层，非 runtime 独有）
- session 集成 **127/10**（从 18 败降；2 例=C 类已判默认资金/采样；
  8 例行为对照族进行中）

## 剩余工作（按优先级）

### A. session 集成 8 例行为对照族（每例判型：语义缺口修 vs 断言适配）

新锚点：全部先跑 `cargo test -p engine --test session -- <名>` 拿首因。

1. `save_restore_keeps_sparse_fifo_keys…`：档往返已过，移至 :4220 的
   original-vs-restored 下一步事件对照——runtime 受理（OrderAccepted
   id=4）vs 旧预期拒收（PriceCageExceeded，市场 bids=[(1,100)] 参考价
   错乱形态）。判型方向：restored 会话的市场参考价读面（空簿回退价 1
   分 vs 旧管线前收）——疑似 restore 后 market best_bid 面缺口。
2. `save_restore_keeps_the_historical_cursor_in_an_empty_book`：
   "new orders must follow the canceled highest historical sequence"
   left=4 right=3——desk 序号推进 vs 测试预期（游标链 3 源 max 的
   runtime 侧差 1）。
3. `partial_fill_never_commits_an_under_reserved_buy_order` /
   `restore_accepts_non_lot_remainders…` / `resting_maker_buy_split…` /
   `resting_sell_orders_reserve…` / `symbolic_buy_releases_excess…`：
   restore(&save).is_ok() 或事件对照失败——大概率同 1/2 的镜像回填/
   游标链族，回填修复后重新取证。
4. `symbolic_highest_resolves…`： Highest 符号价解析对照——用
   `player_session_with_position` fixture。
5. （C 类已判不动：`player_snapshot_excludes…` 默认资金 1e7 旧断言、
   `retail_decision_diagnostics…` 采样前提。）

### B. lib 4 例尾巴（判型已在 MANIFEST 四审尾巴节）

1. `closing_keeps…`（runtime 独红）：公开面 bids=[(Money(1),100)] vs
   预期 [(900,100)]——收盘竞价公开投影对空簿的回退价 1 分（应不展示
   或用前收）。位置：snapshot_inner 的 project_mirror_public_book /
   desk mirror 的 best_bid 投影。
2. `public_step_commits…`（日 K 相位状态机）：install 装载与档校验
   fault2076 的拍前/步后口径两面错位——**协调者已裁定设计级统一超半
   天量级先给方案**。现状：步后口径（与档校验同侧）维持，tick8 日结
   拍 2064 少条=已知面。方案方向：candle 生命周期按"执行相位"状态机
   重述（参考旧管线 finish_market_data 的相位分支逐支复刻），或档校
   验口径改执行相位（需保全量档兼容评估）。
3. `sol7th_foreign…`（两模式红）：身份核 serial 碰撞——A4 修复后外
   来单与计划子单的 (stock<<48|serial) 碰撞使投影误推进（1100 vs
   400）。方向：外来单 serial 与组内计划链 serial 的空间隔离或身份
   核纳入 owner+side 全键。
4. `p2_4b…`（两模式红）：Changed 计数 0 但期末余量断言过——差分计
   数深水；或取证后登记后置（判型理由=计数器捕获路径与
   resting_delta_since_last_echo 的 full_rebuild early-return 交互）。

### C. 其余集成

- auction.rs 2 例 / step_skeleton 3 例（固定 hash=B 类已判）/
  scale_restore 1（B 类）/ publications 1（B 类）/ save_contract 5
  （C 类旧账）——先跑 `--no-fail-fast` 拿当前名单再动。
- weekend（B 类维持后置）。

## 关键坑（本批血泪）

- **fixture 股对**：000001+300001 定稿——**别再动 setup_fixture 的股
  票序**（id 语义已与全部存量断言对齐；600009 主板化是弯路已撤）。
- **打点纪律**：A2PUSH/A2COMMIT/A2FINALS/A2INSTALL 打点全部已撤；
  复用即加即删（`#[cfg(test)] eprintln!`）。
- **lib 全量跑一轮 ≈30-85s**；集成 --no-fail-fast 全量 ≈4 分钟——
  每修一例先单测验证再批跑。
- **协调者裁决点**：public_step 状态机超半天量级先停手上方案；修完
  标准=lib 双模式全绿+集成 32→0（或每例四审级判型登记）+no-fail-fast
  完整回归+独立复核→报新冻结点发五审。

## 台账

- MANIFEST（.tmp/sol-stepframe-red-evidence/MANIFEST.md）四审尾巴节
  已更新至批二；下任继续在彼处记红绿。
- B/C 台账：plans/intentional-divergences.md 四审判定节（19c67408）。
