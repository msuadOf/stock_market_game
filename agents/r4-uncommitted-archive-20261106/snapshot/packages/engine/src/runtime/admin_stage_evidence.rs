//! 多阶段 Admin 的临时 prepared 身份核对；尚未注册生产模块。
//! 本模块不执行资金/股份操作，不认证commit或公司事实来源。
use crate::runtime::world::AdminOperation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PreparedAdminOperation {
    pub stage_index: u32,
    pub operation_index: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum PreparedAdminEvidenceError {
    #[error("阶段或操作数无法表示为协议索引")]
    IndexOverflow,
    #[error("缺少 prepared 操作：stage={expected:?}")]
    Missing { expected: PreparedAdminOperation },
    #[error(
        "prepared 操作身份/顺序不符：位置={position}, expected={expected:?}, actual={actual:?}"
    )]
    Mismatch {
        position: usize,
        expected: PreparedAdminOperation,
        actual: PreparedAdminOperation,
    },
    #[error("多余 prepared 操作：位置={position}, actual={actual:?}")]
    Unexpected {
        position: usize,
        actual: PreparedAdminOperation,
    },
}

/// 仅从发送者保留的不可变载荷借操作；不能用ack自报另一套operand。
pub(crate) fn join_prepared_admin_operations<'a>(
    sent: &'a [Vec<AdminOperation>],
    prepared: &[PreparedAdminOperation],
) -> Result<Vec<&'a AdminOperation>, PreparedAdminEvidenceError> {
    let mut joined = Vec::new();
    let mut position = 0;
    for (stage_index, operations) in sent.iter().enumerate() {
        let stage_index =
            u32::try_from(stage_index).map_err(|_| PreparedAdminEvidenceError::IndexOverflow)?;
        for (operation_index, operation) in operations.iter().enumerate() {
            let operation_index = u32::try_from(operation_index)
                .map_err(|_| PreparedAdminEvidenceError::IndexOverflow)?;
            let expected = PreparedAdminOperation {
                stage_index,
                operation_index,
            };
            let actual = prepared
                .get(position)
                .copied()
                .ok_or(PreparedAdminEvidenceError::Missing { expected })?;
            if actual != expected {
                return Err(PreparedAdminEvidenceError::Mismatch {
                    position,
                    expected,
                    actual,
                });
            }
            joined.push(operation);
            position += 1;
        }
    }
    if let Some(&actual) = prepared.get(position) {
        return Err(PreparedAdminEvidenceError::Unexpected { position, actual });
    }
    Ok(joined)
}
