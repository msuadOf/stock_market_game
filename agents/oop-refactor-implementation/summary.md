# OOP 重构实施与验证

本轮依据冻结的 [调查 summary](../oop-refactor-audit/summary.md)，在 `refactor/oop-complete` 分支落实全部 **128 个动作组及其已接受增强**。低优先级、可选但具有实际状态聚合收益的对象也已实施；调查中明确拒绝的包装不引入。原调查属于历史证据，实施结果在本目录维护，不改写原调查的“尚未实施”状态或签名。

## 完整实施入口

| 区域 | 动作数 | 实际聚合内容 | 逐项方法、调用方、测试与复核 |
| --- | ---: | --- | --- |
| domain | 39 | Account/Position 受控状态、OrderBook 索引、公司账套状态转换、指标递推、个人经历、诊断账本 | [完整台账](domain/completion-ledger.json) |
| pipeline | 20 | 单股执行与跨轮协调、资源预算、受理与结算准备、经历投影、消费和释放登记 | [完整台账](pipeline/implementation-ledger.json) |
| session | 19 | CommittableSessionState、个人状态聚合、计划生命周期、协议 checkpoint、candle 与 attention owner | [完整台账](session/action-ledger.json) |
| frontend | 16 | App 行为 hook、图表与表格资源、Worker/WASM/Remote/Tauri 生命周期、移动投影、请求身份 | [完整台账](frontend/completion.json) |
| hosts | 34 | actor 命令和 Pacing、SessionRegistry、WS publisher、测试 fixture、构建与测量资源 | [完整台账](hosts/status.json) |
| 合计 | **128** | 数据和行为归入真实 owner，调用方同步迁移 | [统一动作状态](plan.json) |

每份台账将需求映射到实际 owner、方法、生产或测试调用方、改动文件及证据；动作内部增强单独核销，不以标题完成代替内部要求。`PositionExperienceTransition` 的六个 dated writer、两个 microstructure accumulator 和 PublicationFactCursor 等内部组合均已落实。正文明确允许省略、且仅增加薄包装的方法保留直接调用，并登记理由。

Rust 采用 `struct + impl`、组合与窄职责借用对象，TypeScript 采用 class 或具名行为 hook。纯计算、DTO、薄入口和独立断言按调查保留，不为增加对象数量建立继承层级。

## 行为和边界

此次重构保持金额分、股数、T+1、累计费用、实际局部受理、P0/P1、P9、RNG 与浮点运算顺序。序列化形状、接受集合、首错及失败后的部分写入也属于兼容边界。

AccountBook 的分页 COW 与缓存失效保留；GameSession 的可提交状态只有一份，测试 hook 与 poison 仍属于 facade；四组机构个人状态聚合后，存档和 hash 继续投影原四字段。没有通过公开任意 setter、DerefMut、默认值或吞错绕过新边界。

各批由未实施者审查完整 diff，新文件纳入全文审查与 SHA 绑定。发现的图表清理部分状态、深层 JSON 接受范围、个人经历子目标遗漏及额外恢复校验等问题已修复并再次复核。最终总审和逐项核销在 [final-review](final-review/) 下维护；[源码冻结清单](source-freeze.json) 绑定本轮 340 个产品、测试和工具文件。

## 实际验证

- engine、server、web-wasm、desktop 的 testlib 编译通过；代表性 fixture 所需的 16 个 integration target 编译通过。
- 四个 package 的全部 target 在默认配置和本次诊断／宿主 features 配置下均通过 `cargo check`，最终零 warning。编译使用 32 jobs，定向链接使用 16 jobs，每条命令受 300 秒进程树上限约束。
- **278 个唯一 Rust 精确 case 全部通过**，包括 237 个 lib case、36 个代表性 integration case 和 5 个 Writer／路径 case。每条命令只运行一个 case，最多 8 个进程并行，每个进程配置 4 个 Rayon worker；全部低于 10 秒上限，最终结果最长约 1.78 秒。移除无调用的过渡方法后，仅重验同源文件的两个既有 case，不增加唯一 case 数量。
- 前端三个 TypeScript project 检查通过；69 个变更 TS/TSX 文件的 lint 通过，零 warning。前端及工具链按分片运行针对性 Node 短测，最终修订补测和历史结果分开登记，不将重复执行相加冒充唯一覆盖数量。
- 两个 WS fixture 在沙箱内无法绑定本机 listener，保留失败记录后，在沙箱外以原断言重跑通过。Writer 初次缺少注册临时目录环境，按已有约定补足环境后通过；桌面 CLI 与 deadline runner 的沙箱限制也保留对照和通过记录。

完整结果及唯一 case 对账见 [final-validation.json](validation/final-validation.json)，前端命令见 [verification.md](frontend/verification.md)，工具链证据见 [hosts 台账](hosts/status.json)。

本轮按用户要求**没有运行复杂回归、浏览器 E2E、长期模拟或性能矩阵**。`hosts-N04` 的周末披露和 `hosts-R2-N05` 的春节旧 fixture 含长周期场景，本轮只做编译和完整 diff 复核；未声称执行了这两个旧场景。也未执行 `compile_fail` doctest，不把短测解释为所有交易路径均已验证。

## 原有工作与交付范围

基线提交为 `b89afb3346743a4b4fccf26c9ac9ff108595f696`。开工前已有的 `AGENTS.md` 修改、`.worktree/` 和 `agents/oop-refactor-audit/` 保持原样，不纳入本轮提交。关键原文件字节校验见 [protected-baseline.json](protected-baseline.json) 及最终验证记录。

本轮没有新增依赖，没有修改 lockfile、生成的 wire 契约或正式交易规则，也没有推送或发布。
