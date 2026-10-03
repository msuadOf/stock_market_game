# Domain 最终核销

39 个动作及权威正文中的增强/可选子目标已实施、独立复核并完成指定短测试。
完整 requirement → owner/method/caller/files/测试与复核绑定见 [completion-ledger.json](completion-ledger.json)。
本组未改 Git 状态、原审计目录或正式文档；所有工作文件均在本目录。

- 新增 owner 均有真实 caller，未以已存在的 dated API 或空结构代替 N07/N32 子目标。
- N07 的 PositionExperienceTransition 已实际服务六个 dated/legacy writer，双 map 不同键集、费用/历史成功点与部分失败面保持。
- N32 两个 microstructure accumulator 已实际拥有方向与报价临时状态；N39/N40 可选动作均已落实。
- 9 个未实施相应源码的 reviewer 完整审查各自 baseline diff 与未跟踪源码，报告含 canonical 身份和最终 SHA；有效发现已修复并复审关闭。
- Market 三个依赖私有异常价格 fixture 的现有用例完整迁入 lib，原十个 case 为 lib 三个及 integration 七个，各一次，断言无丢失。

root 统一构建 `targeted-tests-build-07-result.json` 成功，57.556 秒；最终所有 targets 类型检查
`rust-all-targets-check-08-result.json` 成功，4.258 秒。编译与测试耗时明确分开。
本组相关精确短 case 共 104 个通过：第一批 100 个，加 N07 四个最终双记录保护 case。
两批使用 8 个并发进程、每进程 Rayon 4，case 和进程树 deadline 为 10000ms；未运行完整回归。
运行证据在 [第一批](../validation/rust-lib-short-01-result.json) 与
[增量批](../validation/rust-short-increment-02-result.json)。增量批其他组的环境失败不属于本组
104 个通过 case，本记录不把整份增量 JSON 冒称全绿。

大 A 语义沿用既有官方依据与已登记游戏简化；这批只调整 owner/caller，不新增交易制度、税法
或会计模型。测试与静态复核不证明任意长负载、完整三宿主矩阵或全部接受集合穷举。
本组未完成事项：无。整批总审核与本地 Git 提交继续由 root 处理。
