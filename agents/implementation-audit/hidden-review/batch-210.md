# 批次 210 隐藏来源复核

## 范围与读取

- 计划基线为 `43b1aa5`；本批只审三份既有 hosts/tooling 审计记录。每份均连续读取至 EOF，实际行数和 SHA-256 与唯一计划吻合。
- 对照当前工程规则、现行审计总账、相关 callers/owners/consumers 以及 ADR-0010、ADR-0017、ADR-0020、ADR-0025、ADR-0027。未运行测试、构建或 Git 命令；没有修改源码。
- 本批是历史复核材料的现状校验，不重新认证这些材料之外的源码行为或交易所规则。

## 逐项结论

| 来源 | 现行核对 | 结论 |
|---|---|---|
| `hosts_tooling-closure-final.md` | closure 明确限定为文档绑定、计数和 resolution 的静态复核，并明确不代表测试通过、源码风险已消除或 defect 已修复。其 20 批/294 文件、7 个 action、11 个 unit 与三个 area review 结论属于该 closure 自身记录；本次不把这些计数扩展成当前源码审查证明。 | 范围声明诚实、边界明确；没有新的 A 股规则主张。 |
| `hosts-01.md` | 记录的建议是 desktop/server actor `cmd_tx` 私有化，保留公开 `SessionCommand`；server `event_tx` 由 `subscribe_events()` 提供受控订阅。当前 server `SessionHandles` 内部持有私有 `cmd_tx`/`event_tx`，routes `run_ws` 经 handles 订阅；外部订阅调用见 `tests/actor.rs` 与 `tests/protocol_updates.rs`。`tests/ws.rs` 未发现订阅 sender 的调用。Desktop 命令 sender 同样由 handles 封装；Tauri `ENGINE_EVENT_NAME` 仍属宿主协议常量。 | 当前可见调用链与记录的边界吻合；server setter 与 desktop setter 的速度校验差异仍存在，不能描述为统一。旧记录已说明一次只读 `git diff` 的程序违规，保留该说明是诚实的。 |
| `hosts-02.md` | 路由拆分是组织建议，不是新增 OOP action；server adapter 委托 engine 处理交易规则。当前 ADR-0010 保留宿主协议能力边界，ADR-0025 保留日终存档及 actor-mediated restore，ADR-0027 保留服务 feature/部署边界。文档所列 `run_ws` 注释与处理 Resync、GetFrame、SubmitIntent 的差异作为注释缺陷登记，不得改变协议行为。 | 建议保持最小范围；历史 review 明确其源码边界限制，不冒充完整复核。 |

## 领域与总账

- 本批记录涉及宿主通道封装、WebSocket 协议、部署和审计元数据，不改变撮合、A 股证券规则、交易单位、费用、结算或存档语义；无需新增官方规则依据。
- 当前 engine 复核总账仍将 G09、G38 记为缺口，将 Q02、Q11 保持为开放边界；这些与本批宿主拆分材料无关，不能由 hosts/tooling closure 核销或改状态。
- 未发现需要修订来源复核结论的实质冲突。此结论只覆盖三份来源的表述与本次抽查到的当前调用边界，不宣称源码已重构、相关测试已运行或所有潜在缺陷已排除。

## 来源指纹

| 文件 | 行数 | SHA-256 | EOF |
|---|---:|---|---|
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/hosts_tooling-closure-final.md` | 40 | `389bf01c07f9ee601bc5ad722b9dfad35a85e1295d1c234bf49a5a194160d13e` | 是 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/hosts-01.md` | 57 | `ca41a96710fc42bffeba8db9286b737bda8bd00296503519a52151be87b2395b` | 是 |
| `agents/oop-refactor-audit/chinese-localization/before/exhaustive/reviews/hosts-02.md` | 40 | `18b32126fc82d2496899af1f303b0ddbdd62d3d6014bd0a3a758cc24947f4d82` | 是 |
