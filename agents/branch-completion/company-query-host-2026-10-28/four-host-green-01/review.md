# 四包十八门的静态准备复核

本记录保存未实施 `/root/sol_company_queries_28/query_red_runner_review`（原生工具指定 `gpt-6.1-sol/high`）的实际只读消息结论。此处尚无 green 执行证据；slot／protectedPrior 均 pending，不授权启动。

## 复用与新增范围

复用已实际核准的首红 runner、guard、绝对截止 wrapper、真实 child-status collector。本轮只审四 artifact、十八个 exact 及真实 stdout JSON 取数的新增 delta，不将未变通用监督器重复验收，也不执行 Cargo。

- 四包一次 `cargo test --no-default-features --lib --no-run -j8`，INC0、既有 debug2、唯一 RAM target，不改变 profile 或新增依赖。
- 十八门分母：engine 11、Server 3、Tauri 3、WASM native registry 1。每次 exact 对应真实 unit 注册名、独立 child-status 和精确 `1 passed; 0 failed`，最多八进程并发、每命令进程树最多 10000ms。
- 四 target 按 Cargo `[lib]` 实物分别为 `engine`、`server`、`web_wasm`、`stock_market_game_lib`；记录实际 features／profile／fresh。Server dev dependency 合法开启 `engine/verification-harness`，不得强制 features 为空或外推生产 WASM 图。
- Tauri 沿既有 B/C 的 native `--no-default-features` 验收口径；不是 custom-protocol 打包／发行。WASM 仅 native registry 与共同只读调用，不替真实 wasm32 export／浏览器。
- 实际通过后从 stdout 提取 engine／Server HTTP／Tauri mock IPC 各六种 query 的 JSON，以及 Tauri typed 领域失败一份；任何失败不伪造 fixture 或填空列表。

## 有效发现与修复

1. libtest 的首次 println 可能紧接在同一行 `test FULL_NAME ... ` 后。原 `line.startswith(marker)` 会漏每个 case 的首份 fixture。现仅剥除当前 case 的精确 libtest 前缀后匹配 marker，不扫描任意子串。writer 独立四个极短 probe 验证裸 marker／精确前缀可读、其他 case／噪声拒绝；未运行 runner。
2. 原 Tauri failures 数组直接发布，漏 marker 时可能输出 `[]`。现强制恰一份，校验 `Query → Company → BusinessCondition` 与非空 reason 后发布。

原 reviewer 窄复核确认两发现均关闭，十八 basename 无冲突、四 artifact 映射与真实 println 数量一致，Python AST 有效。没有实际编译或执行。

## 最终来源身份

旧 998 来源已通过静核，随后发现应显式列入 web-wasm `license-file` 的根 LICENSE，另保守列入 Tauri `cfg(windows)` 的 RC／XML 两个实际引用输入。原 reviewer 已核三项引用与实际 hash；将三条移除重构，plan／freeze 精确还原旧 SHA，证明无删项或其他 metadata 改动。补列 Windows 输入不代表执行了 Windows 验收。

当前 **`READY_PENDING_SLOT`：1001 productSources、8 runnerSources、18 case**。

- plan：`ca7c884371221a59fcc54c6619dd66421c7abcad74bf81e7bfbde534d0cefcd1`
- freeze：`0048de521fbdd57e74dcf73ed1489fcf4d401d7e23e5fb334411b020bdb6d927`
- 执行／清理发布／外层仍共享同一 anchor 的 297／299／300 秒绝对截止。

真实前槽、helper HEAD 与实际单件保护状态需在 root 明确交接后绑定并再次窄核。未经实际构建、十八份结果与 JSON／退出收口原件，不能宣称 green 或三宿主交付完成。
