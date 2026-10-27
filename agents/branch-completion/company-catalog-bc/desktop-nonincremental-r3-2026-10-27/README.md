# Desktop 非增量 r3 编译诊断最小修复

r2实际52.961秒Cargo101，两个E0277来自engine_baseline/restore_session中lookup_handles的SendCommandError不能自动转为新BaselineIpcError。原stdout保留完整compiler JSON，不冒业务TDD红。

仅两个call-site增加map_err(map_send_error)，得到原有错误字符串后经现存From<String> for BaselineIpcError进入Existing variant。沿旧From<SendCommandError> for String的同一mapping，不加Serialize、不吞错、不改catalog leaf或fail-stop。现有actor.rs StockCode unused warning在HEAD已有，未顺改。

relative-r2.diff、source.sha256与lib.rs为当前唯一增量冻结。runner/profile/env和原Sol已审INC0 r2相同，保同target、j8、300s/60s/20GiB；未再Cargo，待未实施者窄核和root排槽。
