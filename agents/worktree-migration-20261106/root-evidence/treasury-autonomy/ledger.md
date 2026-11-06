# #118 缺陷台账：回购专户误走自主决策下单通道（P1）

## 1. 缺陷背景与失效机制

- **缺陷编号**：#118（P1 领域语义缺陷）
- **涉及类型**：`crate::account::AccountKind::IssuerRepurchase`（发行人回购专户）
- **历史依据状态（2026-10-22 补核前；保留旧待核记录）**：
  - Leader 先前直接引用《公司法》第 211 条与证监会《上市公司股份回购规则》，**尚未核对原文与适用日期**，本台账撤销该断言。
  - 需要由实现员工按官方现行材料补全：专用证券账户的开立与用途、回购申报的主体与时窗。
  - 当时的接线目标为：回购专户不作为自主市场交易博弈主体，仅由回购方案执行器发出申报。
    这是项目设计目标，不是已经核验的交易所原文，也不证明执行器或时窗限制已实现。
- **当前补核（2026-10-25）**：
  - 来源工作区 final-evidence.json（历史路径：`../../.worktree/treasury-autonomy/.tmp/final-evidence.json`） 第217–246行封存了2026-10-22核对的官方来源、施行日及附件 SHA；Sol 整树审查（历史路径：`../approved-integration/sol-tree-verdict-2026-10-25.md#118-官方原件独立核验及适用边界`） 记录2026-10-25再次在线 HTTP 200、附件 SHA 匹配与原文核对。本次文档实施又直接读取以下两份原件并重算 SHA，一致；未修改旧证据中的 checked_date。
  - 沪市《上市公司自律监管指引第7号——回购股份（2025年3月修订）》，2025-03-28施行：[官方发布页](https://www.sse.com.cn/lawandrules/sselawsrules2025/stocks/mainipo/c/c_20250516_10779134.shtml)、[DOCX](https://www.sse.com.cn/lawandrules/sselawsrules2025/stocks/mainipo/c/10779134/files/0f2fc421931a462698162bb874d830d9.docx)。第21条专户仅存放所回购股份，第36条最终方案/专户后披露报告书，第53条未按报告书实施的监管后果。封存原件（历史路径：`../../.worktree/treasury-autonomy/.tmp/sse-repurchase-rule/rule.docx`） SHA256：`8f05c2678f57f7906ec26b8cb511e5471bb6815d341cd835cc75bbfbd2376ec3`。
  - 深市《上市公司自律监管指引第9号——回购股份（2025年修订）》，依深证上〔2025〕223号于2025-03-28重发施行：[官方重新发布页](https://www.szse.cn/lawrules/rule/stock/trade/t20250327_612564.html)、[修订通知](https://www.szse.cn/lawrules/rule/all/t20250328_612605.html)、[PDF](https://docs.static.szse.cn/www/lawrules/rule/stock/trade/W020250327585488391328.pdf)。对应为第20/35/50条，不能与沪市互用条号；旧2023附件已废止。封存原件（历史路径：`../../.worktree/treasury-autonomy/.tmp/szse-repurchase-rule/szse-rule-current.pdf`） SHA256：`583a2ad0308a9897c276104db11a773b29215852aaf6be0ff7b6b36db8e82c10`。
  - 正式登记目标为集成树 docs/trading-rules.md（历史路径：`../../.worktree/integration-117-118-121-124-126/docs/trading-rules.md#回购专户的自主交易隔离2026-10-25-登记`）；本次不修改主区同名正式文档。规则支持禁止散户式自主交易；`next_due=u64::MAX` 与“仅方案执行器可下单”是项目接线保护/目标，不冒充交易所对游戏架构的规定。显式 `ExternalPlan` 测试只证明运输通路；完整回购执行器、用途及专属时窗/价格限制的端到端校验仍不支持。本次仅补登记，不补签历史或当前 `APPROVED`。

### 详细失效代码链

在 `packages/engine/src/runtime/drive.rs:304-324`：
```rust
let strategy = match id.map(|id| session.account(id).map(|a| a.strategy())) {
    Some(Some(Some(strategy))) => {
        Self::individual_strategy_data(strategy).unwrap_or_else(|e| {
            panic!("个体策略投影失败：{e}")
        })
    }
    _ => Self::baseline_strategy_data(&setup, i), // 专户 a.strategy() 为 None，落入此分支被赋予零售基线策略！
};
let fallback_due = if matches!(
    id.map(|id| session.account(id).map(|a| a.kind())),
    Some(Some(crate::account::AccountKind::Player)) | None
) && ... {
    u64::MAX // 仅玩家豁免
} else {
    (i % 8) as u64 // 专户不是 Player，落入此分支，被安排每 8 tick 到期一次！
};
```
失效后果：
1. 回购专户被赋予普通零售 NPC 策略；
2. 被赋予 `(i % 8)` 的初始到期偏移，后续调度还受 attention 影响，可能走自主买卖链；这不是“每 8 拍一次”的固定周期，实际触发仍需红测确认；
3. 甚至在无任何回购方案的情况下，回购专户自主向市场抛单或买单，严重破坏大 A 证券合规语义与所有权模型。

---

## 2. Sol 独立审查意见（B4d 设计门发现 6）

Sol 审查官指出：
> **发现 6｜专户永不到期**
> 发现本身错了：`IssuerRepurchase` 不匹配 `Player | None`，会走 `i % 8` 并获零售回退策略（`drive.rs:304`）。
> **须显式禁止自主决策，不能只补注释。**
> 这也使"无执行器⇒成交恒空"不严谨：专户可能误走自主决策通道。
> **验收须明确"仅方案执行器可下单、无方案零自主委托"**。
> 验证须跨真实交易拍。

---

## 3. 修复方案边界（员工核实后修正 Leader 草案）

**已核实的关键事实**（2026-10-22 员工回报）：
- `attention_entry` 的 `Some` 分支会**覆盖** `fallback_due`。
  因此**只把 `fallback_due` 改成 `u64::MAX` 不足以禁止调度**——必须同时处理 attention 投影，
  否则专户仍可能在真实的下次 due 时刻醒来。
- `NpcProfile` 需要 `StrategyData`，不能简单塞“空策略”了事。

**边界**：
1. 禁止自主调度必须**双重收口**：策略装配 + 到期入口（含 attention 覆盖路径）。
2. 不扩张 `group.rs` 对 `AccountKind` 的依赖，除非测试证明别无他法。
3. 不新增生产注入 API。
4. 验收口径：**仅方案执行器可下单；无方案时任何真实交易拍都零自主委托**。

---

## 4. TDD 测试要求

1. **红测试（首要）**：
   - 回购专户必须有**非零现金与持仓**——零资产会因“买不起/没票”天然零委托，形成假绿。
   - 用短 fixture 跑 ≥16 个真实交易拍，明确布置 attention 覆盖与到期条件，不把拍数本身当作已触达调度的证据。
   - **正控**：同场的普通 NPC 必须确实产生委托，证明观察通道能看见订单。
2. **断言**：
   - 修复前：专户出现自主委托（红）；
   - 修复后：专户逐拍委托数恒为 0，且每拍重新断言（跨拍持久红线）。
3. **反向验证**：回退禁止改动后测试必须重新变红。
4. **时限**：单 case ≤10s；本轮只跑定向测试，全量回归留到批次终验。
