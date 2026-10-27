# 首次四包编译失败与收口复核

未实施 reviewer `/root/sol_company_queries_28/query_red_runner_review` 窄核实际 stderr、Cargo artifact 原件、`partial-artifact-binding.json` 与 `actual-wait-cleanup.json` 后确认：session `32018` 实际退出 `1`，92.326171 秒；Tauri `lib.rs:240` 为 E0308，未运行十八个 case，目录无 child-status 或 wire fixture，未使用旧 Tauri binary 替代。该批保持编译失败。

Cargo 恰有三个 partial unit：`web_wasm`、`server` 的 features 为空，`engine` 为 `verification-harness`，均为 fresh false。三个 seal 的大小及 inode 与记录匹配、与原 artifact inode 独立；partial 与 actualwait 的 artifacts 数组一致。partial、actualwait、batch 原件均在原 299 秒发布截止前落盘。本次复核没有重新 hash 大 binary。

actualwait 的 owned identities／batch argv 数组为空，reviewer 现场检查观察到的二十三个 PID 均已不存在，scratch 实际不存在；prior 四项记录保持原值。已交 root 指定 E2，green02 必须另开目录。没有运行 Cargo 或 runner。

fresh 源码 reviewer `/root/sol_company_queries_28/company_query_final_review` 同样实核上述编译失败，明确承认此前 v2 静态审查漏掉实际返回类型；拟议最小修复为将 `SendCommandError` 显式转为 `String`，保留中文具体原因。实际修复与 v3 freeze 另行窄核，不能把此前 STATIC 结论当作 green。
