# W3 批独立复核记录（fork 未实施者，2026-10-20）

> 审查范围：`git diff fe9e2ff5..da21236c`（8 提交，54 文件）——两件 B2 补面 +
> ProtocolSession/wasm 绑定 + host 归一层 + G8 schema + 四面板接线 + 台账。
> 完整三问结论由复核 agent 产出，本文件为落档摘要。

## 结论：通过（阻断 0 / 建议 5）

- 大 A 语义逐条对 18725d07 基线核对一致（五态可用性、565 竞价/570-690/780-900
  连续、费用三分项、排他游标、零成交不造 bar、保留链日期连续覆盖、降序分页
  take 100），无凭印象实现。
- u128/u64 双域正确：intraday_average 一致性规则单点化、widen 无静默截断、
  wire 直算不经 u64 会话域。
- ADR-0041 翻译双向一致无漏翻字段（code↔stock 三面请求/响应对称；归一回显
  剥 stock 键再走视图校验）。
- 范围最小：54 文件全落 W3 域（两补面系 main 批准），无夹带。
- 已知发现2（runtime 日K stats 漂移）场景下 web schema 显式拒绝的是内部自相
  矛盾的档——符合铁律二非误伤（根因在引擎侧，已登记）。
- worker-host 六查询 staleness 检查与 queryStockHistory 先例同式，无倒退。

## 建议项（登记后续，非阻断）

1. **分钟时段边界双层不一致（继承旧 main，非本批引入）**：host/market-history.ts
   normalizeMinuteBar 允许 Continuous 690/900（`<=`），save/schema/retained-history.ts
   用 `<`（拒 690/900 Continuous）。引擎发该形态时查询可过、入档被拒（显式失败
   非静默）。engine MinuteBarPhase 注释为 780..=900——后续统一口径并补 690/900
   边界 bar 测试。
2. **stocksById 排序口径**：save/schema/root.ts 用 localeCompare、邻近既有代码用
   默认 sort——六位 ASCII 数字码两者等价；统一写法防未来非纯数字码漂移。
3. **parseActiveMinuteBars 宽容缺键**：当前与引擎全量键构造对齐；建议后续对齐
   book_next_sequences 先例加"键=setup 全集恰好"校验，防引擎改稀疏键后少键档
   静默通过。
4. worker-host 六查询 staleness 检查缺 disposed 条件（initialAllocation 有）——
   与最近先例一致，登记。
5. "观察时刻"文案实为渲染时游戏时钟（注释已声明、tick=0 显式 null 不伪造）——
   建议文案改"时钟"更精确。
