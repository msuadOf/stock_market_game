//! 在任何 worker 启动前校验由可信 AccountBook 构造链提交的专户目录投影。
use super::*;
use std::collections::BTreeSet;

pub(super) fn validate(spec: &WorldSpec) -> Result<(), RuntimeError> {
    let invalid = |detail: String| RuntimeError::ProtocolInvariant {
        detail: format!("专户 binding 无效：{detail}"),
    };
    if spec.admin_treasury_bindings.is_empty() { return Ok(()); }
    let root = spec.root_projection.as_ref()
        .ok_or_else(|| invalid("缺少实际账户名册与股票目录".into()))?;
    if root.catalog.len() != spec.stocks.len() {
        return Err(invalid("股票目录与 World 交易部数量不一致".into()));
    }
    let mut owners = BTreeSet::new();
    let mut accounts = BTreeSet::new();
    for binding in &spec.admin_treasury_bindings {
        let (group, local) = (usize::from(binding.owner.0), binding.owner.1 as usize);
        if group >= spec.n_groups || local >= spec.accounts_per_group
            || usize::from(binding.stock) >= spec.stocks.len() {
            return Err(invalid(format!("坐标越界：{binding:?}")));
        }
        let global = group.checked_mul(spec.accounts_per_group).and_then(|start| start.checked_add(local))
            .ok_or_else(|| invalid(format!("名册坐标溢出：{binding:?}")))?;
        if root.account_ids.get(global) != Some(&binding.account) {
            return Err(invalid(format!("AccountId 与实际 Owner 名册不一致或落入尾部虚位：{binding:?}")));
        }
        let issuer = root.initial_company_facts.issuer_of_stock(crate::StockId(u32::from(binding.stock)))
            .map_err(|error| invalid(format!("{binding:?}：{error}")))?;
        if issuer != binding.issuer {
            return Err(invalid(format!("发行人与股票目录不一致：{binding:?}，实际 issuer={issuer:?}")));
        }
        if !owners.insert((binding.owner, binding.stock))
            || !accounts.insert((binding.account, binding.stock)) {
            return Err(invalid(format!("重复或冲突身份：{binding:?}")));
        }
    }
    Ok(())
}
