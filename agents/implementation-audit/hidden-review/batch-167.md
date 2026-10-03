# 隐藏复核批次 167（owner=2）

## 范围与来源核验

来源根为主仓， caller 基线为 `43b1aa5f25226c72976ca172d32f4a8eeb2272ad`。按计划读取三份历史审计记录全文至 EOF；计划行数及 SHA-256 均匹配，细节见配套 JSON。对照依据包括 `docs/principles.md`、`docs/open-questions.md`、ADR-0010、ADR-0017、ADR-0019、ADR-0027、caller 的 `implementation-audit-2026-10-02.md`、`batch-129.md` 与 `candidate-resolution-04.md`。只做静态复核；未运行测试、构建或 Git 写操作。

## Caller、owner 与 consumer

- `hosts-03.md` 的 WASM 生命周期候选在当前基线已能找到 `SessionRegistry`，与 batch-129 对 `REGISTRY`/`ProtocolSession` 所有权的复核一致；这一历史提案不能作为尚未实施的指令。Server `SessionActor`、`SessionManager` 和 WS `ClientFrameBuffer` 分别保有宿主/连接级所有权，ADR-0010 统一的是 `HostUpdate` 语义，不是底层调度和传输。测试是这些消费边界的 support，不要求额外测试对象。
- 两份 tooling 材料均将脚本/样例作为副作用外壳，将样例 fixture 与 engine 领域逻辑分开。`tooling-01` 的 `WorkspacePathPolicy`、`CaptureArtifactWriter`、`BuildRun` 与 `BuildArtifactPublisher` 属历史候选；batch-129 已确认其中若干对象存在于基线源码，但名称出现不证明行为缺口关闭。
- 工具记录列出的行为问题需要与结构提取分开判断：tooling-01 的 probe 写入失败残留和 build 超时后的进程树/目录清理；tooling-02 的静态 web 打包部分发布、CDP pending 未结算及性能报告子进程停止确认。caller 总账明确登记 G62 的进程树监督缺口，候选裁定 02 也将 deadline 错误表述并入 G62 边界；本批不重复登记该项。当前有限材料未证明其余线索已修复或已获批，也不足以判断为新的 G/Q。

## 语义与裁定

本批来源是工程工具和宿主结构审计，不引入 A 股交易行为、金额/股数单位或新的规则承诺。材料引用的 ADR-0017 现金/股份守恒、ADR-0019 容量边界、ADR-0027 构建分层与项目文档一致；无需以此重新认证交易所规则。

候选反证与边界：source 自称“候选设计，未实施”，审计建议、缺陷线索和验证建议不等同获批实现承诺，也不构成当前源码行为已动态验证的证据。历史 tooling-01 对清理脚本扫描范围的比较是其来源时点观察，不能单独据此要求统一两平台行为。静态打包失败原子性与 CDP/子进程清理线索未在本批 caller 范围内复现，保留供总账独立裁定；不擅自升级或关闭 G/Q。没有发现可确认的遗漏承诺或错误历史核销。

## 来源

- `agents/oop-refactor-audit/chinese-localization/before/exhaustive/modules/hosts-03.md`：75 行，SHA-256 `4a1a6dfb2a41d65086effae32dcb60cab7c111e46e7c99df2c7b0f84d398e168`，读至 EOF。
- `agents/oop-refactor-audit/chinese-localization/before/exhaustive/modules/tooling-01.md`：37 行，SHA-256 `6cf7bbb9de82d67ea7167730a351e7e68133bd2d5bda9fd59ad10d81aa19d0ed`，读至 EOF。
- `agents/oop-refactor-audit/chinese-localization/before/exhaustive/modules/tooling-02.md`：59 行，SHA-256 `a14cc0bbe87dcdecf10bcbe5ee7a62dbac4aceb8703387de1a9f9d288733f8da`，读至 EOF。
