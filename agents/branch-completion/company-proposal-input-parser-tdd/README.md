# PlayerCompanyProposal 输入 parser TDD 证据

- 基线：`e4db69957d6304db30ffe5d5b658cf5efd4ae46c`；分支：`feat/web-proposal-input-parser-tdd`。
- 冻结规格 `fresh-redo-spec.test.ts` 原样复制到正式测试文件，SHA256 为 `d22b36c6441d6b0797ca6980bf8ad1e598d87155cdf2814058897a79b95d3c5b`。
- `red-v1/` 保留同字节 stub、测试快照、stdout/stderr、退出码、命令和 helper SHA。测试 11 项中 10 项业务失败、1 项因该冻结 case 的 throw 断言允许任意错误而通过；不是模块或 fixture 路径失败。
- `green-v3/` 是最终实现后的三 job 并行验证记录。Node 测试 11/11，窄 strict TypeScript 检查、目标文件 oxlint 均退出码 0；每个命令进程树 deadline 为 10000ms。
- deadline 使用真实 main helper：`/data1/baiyifan/workplace/stock_market_game/.worktree/main-merge-2026-10-25/scripts/run-with-deadline.mjs`，SHA256 `de7fd2a82bc66e7a62fbc487002727b71ad28b69c1e645f82fa84bbfdb068916`。
- 复用已有 `apps/web/node_modules/.bin` 工具，无依赖安装、无依赖目录改动。
- parser 仅校验六类输入 serde shape 和整数 wire 格式，复用 `parseMoney`；它不审批、不伪造 Outcome/Receipt，也不接入 API、App、存档或 Rust。
