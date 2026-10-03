# 批次 076：历史复核材料再审

## 覆盖与基线

按 `agents/implementation-audit/hidden-review/scan-plan.json` 读取 batch 76 清单。目标源码基线 `HEAD` 为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。逐篇从首行连续读取至 EOF，行数及 SHA 均与清单相符；下表记录来源章节族。来源为历史审计/复核，不是实施指令；本批只抽取 OOP 候选及核对当前状态，不修复产品或工具缺陷。

| 来源 | 行数 / SHA-256 | 章节族 |
|---|---:|---|
| `agents/oop-refactor-audit/exhaustive/reviews/hosts-03.md` | 38 / `b8fe95ea7ba9b0b2c80e63eb38cc4e0551b6c49f0fdf7bcd6c644c468f263769` | 逐文件核对；跨层检查；hosts-03 delta 两轮独立复核 |
| `agents/oop-refactor-audit/exhaustive/reviews/hosts-area-final.md` | 31 / `9fe178a7cbdad8ea606f4322b1f85358fed986b8b4f377229478910221b31590` | 绑定材料；区域整合核对；三门结论 |
| `agents/oop-refactor-audit/exhaustive/reviews/root-audit-tools-final.md` | 62 / `70bbc85d1d244f28901e0f0a3b9ca997a13081d7c987a06a335a26d93f7841a4` | 范围与方法；文件指纹；清单/索引；结果与 closure；工具边界及门禁 |

已读工作树 `AGENTS.md`、`docs/principles.md`；对照当前 G01–G68/Q01–Q23 主账、`docs/open-questions.md`，以及涵盖相关宿主/工具边界的 ADR（重点 ADR-0010、0017、0027）。项目原则要求保留 engine 交易权威、诚实陈述候选状态并由未实施者独立复核；OOP 审计不构成行为修复授权。相关历史独立复核材料明确将 `SessionRegistry` 的句柄耗尽缺陷与等价聚合拆开，不能把首次未通过段落当最终裁决。

## 当前代码与旧结论再证

- hosts-03 的唯一 OOP 候选是 WASM worker-local `SessionRegistry` 生命周期封装。当前 `apps/web-wasm/src/lib.rs:43-115` 已有 thread-local registry 与 `SessionRegistry`，`create`/`restore`/`with_session`/步进/删除路径在 `:331-350`、`:473-529` 使用它；这条历史候选已经在产品树落地，不应作为待实施动作重开。engine `ProtocolSession` 仍承载会话/交易行为，registry 不引入交易规则或单位变化，无新增 A 股规则主张。
- 旧复核指出 `NEXT.fetch_add` 回绕后可能覆盖活句柄；当前 `apps/web-wasm/src/lib.rs:57-62` 仍采用 `fetch_add` 并直接登记。该边界仍由现行 Q20 登记（`agents/implementation-audit/implementation-audit-2026-10-02.md:158`），不能因 Registry 已存在而核销。来源第 25 行首次 delta 复核指出的混合范围问题已由第 30–38 行二次复核拆分修订：耗尽拒绝/防覆盖和注入计数器测试是独立 defect lead，不是此次等价封装的要求。最终旧结论有效。
- hosts 区域复核的三条候选分类与后续状态一致：desktop sender 边界、server 受控 sender/只读订阅边界和 WASM registry 生命周期；当前 caller 位置由既有基线核对记录精确定位于 `apps/desktop/src-tauri/src/actor.rs:333-336`、`apps/server/src/actor.rs:606-625`、`apps/server/src/routes.rs:1269-1275,1414-1420`、`apps/web-wasm/src/lib.rs:43-115,331-350,473-529`。候选结构落地不能推导为行为缺陷关闭：固定倍率逐 tick 发布仍记 G19（主账 `:40`），句柄回绕仍记 Q20。区域结论也准确将 routes 子模块拆分列为组织说明，而不是 OOP 动作。
- root audit tools 最终复核明确限定为审计工具、完整元数据、导航和版本绑定，并声明没有重读 1186 个源码文件或重新认证其语义（来源 `:9-11`）。因此其“通过”只支持所述元数据核验，不可扩大成产品实现或交易规则验证。当前主账仍将性能采样异常收尾归 G61（`:129`），嵌套进程树监督归 G62（`:130`）；这些属于行为/监督缺口，不是 OOP action。来源对哈希、full_read 与独立性证明边界的披露（`:60-62`）诚实且必要。

## 判定

1. **大 A 语义与依据：通过。** 三篇材料复核的是宿主所有权、工具审计元数据与生命周期，没有引入或模糊撮合、委托、现金/股份、T+1 等规则。无需新交易所依据；不以此审查替代规则核验。
2. **必要性与最小范围：通过。** 已落地候选不重复登记；routes 组织建议不升级为 OOP；Q20、G19、G61、G62 保持各自缺陷范围，不以对象存在冒充行为修复。
3. **边界、跨层语义与复杂度：通过。** 独立复核二次结论正确拆分了句柄回绕修复；`step_frame` 后 `tick_batch` 仅保证顺序、不承诺组合原子性，需另行设计；工具最终复核的元数据认证边界也被明确陈述。未发现本批需新增的候选或 G/Q 映射。

未改产品文件，未运行测试、构建或回归；未执行 Git 写操作。交易语义没有变化。
