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
    #[error("阶段 prepared 身份核对尚未实现")]
    NotPrepared,
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
    _sent: &'a [Vec<AdminOperation>],
    _prepared: &[PreparedAdminOperation],
) -> Result<Vec<&'a AdminOperation>, PreparedAdminEvidenceError> {
    Err(PreparedAdminEvidenceError::NotPrepared)
}
