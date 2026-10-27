# Proposal 输入 parser 独立复核

本轮复核者未实施该 parser，未修改其 worktree。限定结论：`e4db69957d6304db30ffe5d5b658cf5efd4ae46c..6b1800bc1faa98d006d3996cee1cf8d478b5ffc3` 的输入 parser 功能、最小范围及本次重新实施的 TDD 原件通过；不代表生产 command、三 host、UI、Outcome 或完整 B/C 通过。

- 正式实现提交为 `84173142`，后续 `6b1800bc` 只补充首次 TypeScript 失败原因说明；实际 tree clean。
- 已审完整正式源码 diff、最终两个正式文件及本主题关键红绿原件。正式源码 SHA `654337fbfc9e6fc24ac140ec3587a219de9aafda8b3d707428e82d018772a6fc`；冻结测试 SHA `d22b36c6441d6b0797ca6980bf8ad1e598d87155cdf2814058897a79b95d3c5b`；实际 red stub SHA `9469b4ac4e396e18d00910973ccc55ee45a8281e3f68f4006b5e93dbd779d210`。最终 manifest 四项逐项核对通过。
- 原 red-v1 确为 1 pass、10 fail，失败由已存在可编译 stub 显式抛错触发；不是缺模块。一个只检查抛错的 case 对 stub 通过，原记录已明确，不把该 case 当有效红。测试快照与最终规格同字节。原 green-v1 TypeScript 失败及 green-v3 修复日志均保留。
- 独立重跑 Node 11/11，strict TypeScript exit 0，目标 lint exit 0；三个 job 并行，均以绝对 main `run-with-deadline.mjs` 外部监督 10000ms，Node 增加 `--test-timeout=10000 --test-concurrency=4`。运行 Node24，复用已有工具，未安装依赖。原执行输出在本轮 tool 记录。
- 六个变体、字段、Money 分字符串、canonical u64 字符串、u32、三个嵌套 enum 均与 `company-b4e-commands/packages/engine/src/session/company_commands_types.rs` 及真实域 enum 逐字核对。测试中的 Rust fixture 为已冻结值的内嵌副本；本次没有重新运行 Rust producer。

三问结论：此片保留 A 股领域既有游戏化提案简化及单位，只验证 serde 输入，不赋予审批权限；范围仅两个正式文件和主题证据，无 API 或全局 helper；上下界、负零、缺字段、多字段、错误 enum 与无副作用已有覆盖，未见必须修复项。真正受理、资金约束、持仓、治理及发布均留给实际生产入口，不用格式有效冒充业务成功。
