# PlanId 独立身份核心批（2026-10-25）

## 公开接口与范围

- `PlanId { owner: AccountId, sequence: u64 }`，数值排序为 owner → sequence；wire 两项均 canonical u64 十进制字符串。
- `OwnerSequence { owner, next_sequence: Option<u64> }`，wire 必填 `next_sequence`，string 或 null；None 为耗尽。
- `PlanBook::from_parts(policy, Vec<OwnerSequence>, Vec<TradingPlan>)` 在建立 map 前查重和校验。
- `PlanBook::owner_sequences()` 按数值 owner 顺序遍历；`into_parts()` 完整保留 policy、terminal 与高游标，供 runtime 下一批接线。
- 新格式 `{policy, owner_sequences:[...], plans:[...]}`；严格拒绝旧结构，无迁移或 fallback。
- PlanRecords 改 16 字节数值 radix 键；group Abort hash 覆盖两个身份分量、policy 和全部游标及 None。
- 本批只改核心、Rust 机械调用点与测试，不实施 runtime 权威播种/路由/完整合并，也不实施 #122 三档策略。

## TDD 与验证

- 基线 f5820291；红测提交 d6c3ff43。
- 新增四项红测首次编译实际失败，出现 OwnerSequence 未定义、PlanId 无 owner/sequence、无 sequence 字段等九项错误；原件 `plan-identity-red-2026-10-25.log`。
- 核心实现后同测试二进制普通执行：外监督10000ms，`--test-threads=4`；4 passed，0 failed，0.00s；原件 `plan-identity-green-2026-10-25.log`。
- 构建单独归类为长编译，外进程 runBoundedCommand 300000ms；CARGO_BUILD_JOBS=16，独立 `.tmp/plan-identity-target`；绿色构建用61s。cold build deps 并行；单 crate Rust frontend 存在工具串行阶段，不能以 jobs 数冒称全程满核。
- 原始日志保留在本目录、受仓库 `.log` 忽略规则影响，未伪装成跟踪文件。完整受影响 Rust 测试编译/回归、bindings 导出及独立复核尚待后续补齐，不能凭本阶段宣称身份前置完整交付。

## A 股语义与错误上下文

本批没有新增交易制度，沿用 trading-rules、ADR-0015/0016/0017/0025/0031/0041/0043 已登记依据；只改内部身份与当前格式，资金分、股份股、T+1、费用与受理 FIFO 不变，未声称重新联网核验交易所规则。

PlanId 错误上下文统一为 owner 高/低 u32、sequence 高/低 u32。现存错误码2141/2142/2143/2144/2183/2184/1042/2185 的身份前缀由一个槽改为四槽，其后字段保持原义；后续将更新正式错误清单。新 PlanError 为领域类型化错误；运行协议映射由 runtime 批实现。
