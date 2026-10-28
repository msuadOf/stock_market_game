# 四包 native 十八门的实际独立终审

未实施 runner reviewer `/root/sol_company_queries_28/query_red_runner_review` 结论为 `VERIFIED_NATIVE_GREEN18`，未实施 fresh 产品 reviewer `/root/sol_company_queries_28/company_query_final_review` 结论为实际四包 native `18/18 GREEN`。两人均未运行 Cargo、未修改源码。

实际 session `57868` 退出 `0`，批次 19.355659 秒；十八个冻结 exact 均有真实 stdout／child-status、child exit 0、精确 `1 passed; 0 failed`，注册清单、argv 的 target 与二进制 SHA 一致。最长 child 约 1.684 秒、含 supervisor 的命令最长 1.824 秒。1001 项 before／after／freeze／executed plan 一致，当前逐项 hash 未漂移，八项 runner 与十四项 v3 产品 hash 匹配。实际 plan SHA `8631cb1593a32d6e1add75b84829d7d04e45ffbd3b050f68fce5f7261079a816` 与 guard／batch 回执一致。

原始 Cargo JSON 恰有四个 lib test unit，build-finished success true：WASM／Server／engine fresh true，分别与 green01 认证的同源 partial SHA 相同；Tauri fresh false，为修复后真实新 unit `0e1c0b80b0ce7e6e2b2e8233bc793ada7398094a92c097833afe8188764bf9e3`。features／profile 原样记录，engine 的 `verification-harness` 来自 Server dev dependency，其他 features 为空；均为原 debug2／test profile。fresh 产品 reviewer 独立核四个 sealed 单件完整 SHA、大小与回执一致，没有拿可能已被后继覆盖的当前 target 冒充证据。

两 reviewer 均从十八个 case 的真实 stdout 精确重提取 JSON，再与 wire 文件逐对象比较：engine／Server HTTP／Tauri mock IPC 各恰六种 query，owner 为字符串 `"0"`；Server／Tauri generation 为字符串 `"1"`。Tauri failure 恰一份 `Query → Company → BusinessCondition`，含尚未结算的具体原因。engine 的非空税账／Flat receipt／Rights entitlement 隔离 fixture 与宿主合法空列表范围均如实保留，另有实际非零 member 的 actor 测试，不用空结果冒充 owner 隔离。

资源 guard 实际 j8／INC0，二十个样本最多二十六线程、每 500ms 最多 301 CPU ticks，最小 RAM 余量约 77.083GiB。owned／argv 数组为空，观察到的七十个 PID 已不存在，scratch 实际无；回执早于 299 秒发布截止、无 deadline failure。E2 当前 default `21ffb460…` 在本批共享窗先独立封存；plan 中 W1 inventory 仅为历史记录，未当作当前 default 身份。

fresh 产品 reviewer 对 AGENTS 三问的结论保持：沿既有 A 股领域方法与精确单位，无交易制度变化；注册、认证身份与公开错误边界为需求所必需；未发现未修复的边界遗漏或跨层漂移。此前 STATIC 漏掉的 Tauri 类型错误及实际首次失败仍完整保留。结论不覆盖 WASM export／浏览器、桌面打包、TS 跨层消费或生产公司结算；最终 README 与证据提交仅待 metadata 窄核。

最终 metadata 窄核已 `APPROVED`：九十五个暂存路径均属本主题，零产品 delta，暂存与工作原件字节一致，无二进制、可执行文件、symlink 或常见真实凭据模式。三十三份新增／修改 JSON 可解析，另两份既有 cases／freeze 合计三十五份本轮 JSON；七项关键执行／wire SHA 保持前轮实审值，handoff 与 plan／slot binding 匹配。唯一报告精度发现已关闭：Server 行明确与 engine 的本人隔离差异正控组合验证，不把 Server Flat 空税籍结果宣称为跨 owner 差异事实。reviewer 确认完整 v2 审查、v3 唯一修复、真实 green02 与最终证据门均闭合，可提交 clean OID 交 root。
