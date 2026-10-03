# Batch 004：前端源码挑战历史结论复核

## 范围与来源完整性

本记录只复核 scan-plan id=4 的三份历史材料，并以指定产品基线 `43b1aa5` 的源码核对四项候选。历史文档里的实现指令只作为审查材料，不作为本次实现授权。未修改产品文件，未执行 Git 写入、测试或回归。

| 来源 | 声明行数 | SHA-256 | 实际读取 | EOF |
|---|---:|---|---|---|
| `agents/oop-refactor-audit/challenge-2026-10-03/frontend/report.md` | 386 | `015ee906af21b3a61650ded908eb3130251b253bd1443b5c6c2e47c4944fb84e` | 连续分段全文读取，1–130、131–260、261–386 | 是 |
| `agents/oop-refactor-audit/challenge-2026-10-03/frontend/review-findings-61.md` | 175 | `00296fa0ad48a99966fe51f4b925e1da215e7e2a26ebf7f79f9b620388f59ddb` | 连续分段全文读取，1–100、101–175 | 是 |
| `agents/oop-refactor-audit/challenge-2026-10-03/frontend/review-model-correction.md` | 5 | `ead224db4bc0fcccfffa484048d6c70f62fa107006b6cf0422711be02a9be099` | 全文读取 | 是 |

以上行数和摘要由当前原路径复算，与计划元数据一致。首次读取因输出截断未完整，之后已按区间补读至 EOF；这次重读覆盖了全部三份材料。

## 章节矩阵

以下矩阵用于确认本批候选与实现审计总账的关系，不重新认证总账中各缺口状态。分组取自 `implementation-audit-2026-10-02.md` 的功能章节及 `coverage-index.md` 的代码复核映射。

| 总账章节 | 所含 G / Q 范围 | 本批材料关联 | 本次裁定 |
|---|---|---|---|
| 2.1 宿主与远程链路 | G01–G05、G18–G20、G40、G53、G66 | N02 RemotePublisherState；明确涉及 waiter / dispose 的现存边界 | N02 已实现在基线；单槽 waiter 覆盖问题仍属于 G66，N02 没有修复它 |
| 2.2 策略、个人信息与估值 | G06–G09、G20、G42–G43；Q02、Q11 | 未发现本批候选与策略 / 个人信息状态 owner 有同一调用方或数据所有者 | 无直接关系 |
| 2.3 行情显示与日历边界 | G10–G15、G44、G46–G47 | N01 管理计算请求身份；不拥有指标公式或 A 股成交、价格、成交量规则 | 无交易语义改变；不构成上述 G 的实现或核销 |
| 2.4 工程、交互和发布 | G16–G17、G21–G26、G45、G48–G52、G54–G56、G64–G68；Q01、Q04–Q09 | N01/N03/N04 是请求 / IPC 时间线状态，不触及这些正式契约 | 无直接关系 |
| 2.5 补充逐章核对发现 | G28–G38、G41、G49–G52、G57–G59、G64–G68；Q12–Q23 | 四候选不是这些新缺口的同一 owner；N02 的已知 waiter 边界除外 | 未发现可据本批材料新增或核销的 G/Q |
| 2.6 公司与计划生产闭环补漏 | G28、G35–G38、G41–G43、G58–G59、G67–G68；Q02、Q11–Q23 | N04 只管理公开报告查询 ticket；不拥有报告事实、披露口径或策略链 | N04 在基线已实现；不核销公司 / 计划闭环条目 |
| 2.7 验收工具契约 | G26–G27、G39、G60–G63；Q05 | 历史 frontend report 的测试盘点不是本轮测试结果 | 未执行测试；不对工具缺口作状态变更 |

章节中重复出现的编号代表不同交叉复核主题，按总账原文保留；本矩阵不将重复编号解释为新增缺口。完整编号命名空间提醒：本报告 Q01–Q23 是实现审计内部 ID，与 `docs/open-questions.md` 的 Q 编号不同。

## 候选与当前源码证据

历史 `report.md:15–84`、`:86–179`、`:181–257`、`:259–303` 分别提出 N01–N04；其总体 4 项 new 的结论见 `report.md:5`。独立复核 `review-findings-61.md:142–175` 再次确认这四项分类，并记录四条历史修订见 `:100–104`。产品源码则显示这四个名称和职责已在给定基线落地，因此不能把它们作为当前待实施缺口报告。

| 候选 | 基线调用链 / 代码位置 | 与历史结论的比对 | 现存边界 |
|---|---|---|---|
| N01 `IndicatorResultRequest` | `.worktree/implementation-reaudit/apps/web/src/components/indicator-results.ts:44–82` 定义请求身份、引用匹配、pending/resolve/reject 转换；`useIndicatorResults.ts:18–45` 由 hook 持 gate 与 React state 并接 Promise。 | 候选已实现；实现保留 React 为呈现 state owner，匹配历史狭义边界。测试 `indicator-results.test.ts:42–63` 覆盖身份、过期响应、解析失败和 reject；未见 hook 级 React 生命周期测试于该测试文件，故不能把组件级集成行为称为已验收。 | 不改变 Rust 指标算法。A 股价格/成交量输入语义继续来自宿主输入构造，不由请求身份类裁决。 |
| N02 `RemotePublisherState` | `remote-publisher-state.ts:8–71` 拥有 socket、connection generation、awaiting baseline、单槽 waiter、baseline cache/epoch；`remote-host.ts:46–73,75–115,117–149,201–211` 为消费 caller。 | 候选已实现，且状态转移总体按历史限定由 adapter 编排；`remote-publisher-state.test.ts:11–59` 覆盖身份与 cache epoch、waiter 覆盖/重入。 | `beginBaselineWait` 直接替换唯一 waiter（`remote-publisher-state.ts:31–34`），旧 waiter 不会自动 settle；dispose 只拒绝当前 waiter 且未 reject 命令 registry（`remote-host.ts:168–178`）。这与总账 G66 描述吻合，是仍开放边界；抽取没有消除它。旧 socket `onerror` 仍直接 `fail`（`remote-host.ts:129`），亦不应从 identity 管理推导成已防护。 |
| N03 `TauriTimelineState` | `tauri-timeline-state.ts:19–96` 拥有 timeline、generation、cache、epoch；`tauri-host.ts:97–125` 初始化并供事件 listener 检查。 | 候选已实现；安装方法保留 generation 校验、timeline 写入及 snapshot 解析的先后关系。测试 `tauri-timeline-state.test.ts:7–40` 覆盖大 generation、解析失败部分更新与 dispose 后旧 epoch 检查。 | 不合并 session/listener 生命周期或 IPC I/O，不改变 engine authoritative state。测试代码存在不代表本轮运行或覆盖全 listener 失败组合。 |
| N04 `CompanyRequestRegistry` | `company-request-registry.ts:1–23` 实现 sequence、keyed ticket、force 替换与条件清除；`company-query-coordinator.ts:55–81,84–142` 为组合 owner 和 page/by-id callers。 | 候选已实现；coordinator 仍拥有 generation/disposed/cache/dispatch，registry 未复制整体查询 authority。测试 `company-query-coordinator.test.ts:303–351` 覆盖 force 后旧请求与独立 page/by-id ticket。 | 管理请求身份，不拥有公开披露、报告内容或 Redux 数据；不改变公告披露窗口和公开报告语义。 |

## 证据结论

- **历史分类质量：** 在其冻结时间点，四项均按窄子 owner 提案，不把既存完整 class/hook 再报一次；N02 与 N04 的边界划分合理。历史报告明确注明测试静态读取、待补范围和“非强制”性质，没有声称候选已实施。
- **基线状态更新：** 以 `43b1aa5` 为审查基线，四个候选实现类均可定位，caller 也已切换到组合对象。故目前结论应更新为“已实现的内部 owner”，而非仍待实施的 OOP 候选。材料未提供实现提交时间线，本记录不推断其实现提交或发布日期。
- **G/Q 交叉核对：** 未发现四项提案意外覆盖或核销 G01–G68/Q01–Q23。N02 公开承认且代码仍保留的单槽 waiter / dispose pending command 问题与 G66 相交；它是已知未解决边界，不是新回归，也不能因有 `RemotePublisherState` 或测试而标完成。
- **大 A 语义：** 本批没有新交易规则或 engine 领域行为。N01 不拥有指标公式；N02/N03 传递完整权威 baseline / timeline 身份；N04 仅关联公开报告查询。未发现价格单位、股份单位、成交撮合或披露窗口语义漂移，亦不以本审查替代官方规则核对。
- **范围与测试：** 没有代码改动，也没有运行测试。现有测试源码仅作为覆盖边界证据；不能据此宣称行为通过。后续若修改 G66 waiter/取消语义，应独立明确请求终态和未知执行结果，再按 TDD 实施和复核。

## 唯一产物声明

本批审计唯一新建文件为本 Markdown 与同目录 `batch-004.json`；未写产品源码、测试、正式 docs 或其他工作文件。
