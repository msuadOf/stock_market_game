# Shared market grant lifecycle 边界复核

- 范围：只读审查 Server `SessionManager` 创建/恢复/重置路径与 Native SQLite 控制 grant 生命周期；判断新经济局是否错误继承不同市场权限。
- 依据：ADR-0030/0033、Server 启动选项文案、当前 API/DB 对市场身份的建模。
- 结论：**按当前已定产品模型未确认存在 bug。** 实现和接口只建模一个 `shared-market` 安全域；`new-market` 是在该安全域中跳过旧经济存档并等待创建新经济时间线，不是创建一个有独立授权命名空间的第二市场。ADR-0033 与 CLI 帮助均明确 `--new-market` 保留既有控制授权；加载/重置经济状态也不改 grant 符合已决规则。

## 生命周期证据

- `market_control_grants` 主键为 `(market_id, subject_id)`，授权与 `archive_slots` 分表保存；`grant_market_control` 是幂等插入，当前没有撤权或按新局清空 API。
- `SessionManager::resume_session` 从指定槽恢复 `SaveSlot` 后注册 actor。`register_game` 统一读取 `market_controllers("shared-market")` 恢复授权；因此进程重启、加载存档和切换经济档保留相同安全域的 controller。
- `new_shared_session` 仅在当前 manager 没有内存市场时创建新 engine；创建者绑定为成员。注册 actor 时先加载该固定安全域的既有控制者，再幂等授予新创建者。因此若以 `--new-market` 启动且用旧数据库新建经济局，旧 controller 仍有控制权，新创建者也成为 controller。
- actor `Reset` 用新 `ProtocolSession` 替换经济时间线，controller 集合不变。这属于同市场 reset，与“加载其他日终档保留当前授权”一致。
- `/api/new` 在已有共享市场时返回 `MARKET_ALREADY_EXISTS`；当前服务 API 没有创建多个具有不同 `market_id` 的并行市场能力。CLI `--new-market` 文案明确跳过启动存档但保留既有账号与控制授权。

## 产品边界

“新经济局仍沿用同一 shared-market 的授权”有现行实现与 CLI 文档依据，不能把加载存档保留权限的决定误解成应当为每个新局撤权。若产品将来要求“新建独立市场并且默认只有该市场创建者有控制权”，则需要先明确新市场与旧市场的可达性、授权撤销/保留策略和 SQLite market identity 生命周期，再实现独立 `market_id` 并据此划定 grant；当前材料未定义该模式。本次不据此删除既有 grant 或报告代码缺陷。

未运行测试；本项为源代码与已决文档的边界审查。
