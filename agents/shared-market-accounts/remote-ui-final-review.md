# Remote UI 整批独立复核

## 范围与依据

复核对象为当前工作区未提交的 Web Remote UI、启动／生命周期、RemoteHost 接线与相关测试完整 diff；未实施改动、未编译、未运行测试。已阅读 `AGENTS.md`、`docs/principles.md`、ADR-0030、ADR-0033 与 `remote-ui-wiring.md`。

重点检查登录／guest／credential IndexedDB、RemoteHost 的身份与所选市场 context、本人 `account_id` selectors、无资金账户 Controller 入口、日终档案加载和选槽、generation/lifecycle 隔离、入场资金设置与市场重置。

## 结论

- 当前复核未发现 Remote UI 将账户权限扩展到其他主体、从资金档案恢复市场控制权，或把缺席主体静默加入并发放资金的确定性问题。
- 登录和 guest 均须明确触发；启动 RemoteHost 要求身份 token 与已选择的 context；代码没有借旧 session token、固定账户 `0` 或隐式建局绕过认证／成员关系。
- `account_id` 保持规范 u64 十进制字符串，用于本人资产与委托路径；缺席账户 selector 返回空，不冒充账户 0。无资金账户但有控制权的主体存在公共市场入口，交易接口要求本人资金账户。
- 档案加载、重置及重新加入均有明确 UI 动作；选槽在恢复后执行，generation 检查隔离迟到操作。入场资金设置表达为只影响以后新加入主体，金额单位为分，未改变沪深 A 股撮合、T+1、费用或股份单位语义。
- 主要错误路径有用户可见反馈；本次静态复核没有发现 credential 被 UI 主动写入游戏档或密码持久化的路径。

## 必须保留的验收边界

- 本结论是静态 diff 复核，不是构建、测试或真实 Server 验收。`remote-ui-wiring.md` 明确 mock WebSocket/fetch 不构成多客户端端到端证据；不得据此宣称真实多客户端 Server 市场、共享账户或持久化已验收。
- 复核未验证 Server 对 REST／WebSocket 每条受保护路径的权威授权、跨客户端恢复与并发冲突；这些属于完整整批 Gate 的 Server／Engine 复核和真实集成验收范围。
- 当前未发现需作者修复后复核的确定性 UI/A 股语义问题；整批最终完成仍取决于 root 汇总其他层复核与实际验收证据。
