# 完整产品源码预审（v2）

未实施 reviewer `/root/sol_company_queries_28/company_query_boundary_review` 由原生工具指定为 `gpt-6.1-sol/high`。其全程只读，未运行 Cargo、未修改产品。

最终结论为 **`STATIC_READY`，可排四包实际编译验证，不代表 green／JS／终验**。

实核 manifest SHA `b9e7da45fb2c0f2c37be665d173272688a90f02f5c3d78eec980ea16347767e1`、完整 diff SHA `66727c2d8afc769658194437502a910d23dc9438d3e9f5e75bc2286c1cd3176c`；14／14 产品文件 SHA 匹配。按完整 base `8e5b32de305257b7cb9aeb284d19551cdf3ce2c3` 与十四路径重算的 1536 行 diff，字节与冻结文件一致。

v1 有效发现已修：

- 非零 Server 成员的 Flat 税籍与账户 0 相同，单靠税籍不能证明 domain owner 参数正确。engine Flat 非空测试现创建真实其他 member，回执 100／200 分、实际查询其他 member 并核其账户与金额，与 0 不同；结合 Server 当前 membership 的 owner 转发强正控构成差异事实与宿主身份的组合证据。不新增公开 game_mut／生产注入 API。
- 三宿主原只查一种 kind，现 Server 真 HTTP、Tauri actor、WASM native registry 均覆盖六种；Tauri 再用现有 `command_builder`／`get_ipc_response` 走真实 mock IPC 六种及 typed reason／旧 generation 拒绝。WASM 真实 export／serde_wasm_bindgen／浏览器仍交 BC/W1 实际制品验收，native helper 不冒充该门。

相对 v1 六个变动路径全是上述必要测试，生产源码 SHA 全部保持。Server dev graph 启用 `engine/verification-harness` 为合法 Cargo 事实，后续 artifact 不强制 features 为空。

AGENTS 三问：薄投影不改 A 股制度、税算法或精确单位，不因 Flat 简税而模糊真实规则；owner 外壳、具体安全 reason 和三宿主注册为需求必要最小范围，无新依赖／算法／存储／World；原身份与出口覆盖缺口已按测试范围修复，真实 green、wire stdout、BC/W1 及 fresh 完整终审仍待完成。
