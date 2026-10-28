//! 公司批准只安装当前库与 facts，不制造观察点或投资者资产操作。

use super::RuntimeDriver;
use crate::runtime::protocol::{InformationPreparePayload, RuntimeError};

pub(crate) struct CompanyApprovalFailure {
    pub error: Box<dyn std::error::Error + Send + Sync>,
    pub terminal: bool,
}

impl RuntimeDriver {
    #[cfg(test)]
    pub(crate) fn fail_company_approval_commit_for_test(&mut self) {
        use crate::runtime::protocol::{AdminFault, AdminPhase, GroupIn};
        let (temporary, receiver) = std::sync::mpsc::channel();
        drop(receiver);
        let real = self.world.swap_group_sender_for_test(1, temporary);
        real.send(GroupIn::AdminFaultForTest {
            phase: AdminPhase::Commit,
            fault: AdminFault::WrongId,
        })
        .expect("真实组通道必须接受 Commit 故障注入");
        self.world.swap_group_sender_for_test(1, real);
    }

    #[cfg(test)]
    pub(crate) fn disconnect_company_approval_for_test(&mut self) {
        let (disconnected, receiver) = std::sync::mpsc::channel();
        drop(receiver);
        self.world.swap_group_sender_for_test(1, disconnected);
    }

    pub(crate) fn commit_company_approval(
        &mut self,
        information: InformationPreparePayload,
        inspect: impl FnOnce() -> bool,
    ) -> Result<(), CompanyApprovalFailure> {
        let groups = self.world.n_groups();
        let expected = information.clone();
        let result =
            self.world
                .with_information_transaction(Vec::new(), information, |candidate| {
                    if candidate.groups.len() != groups {
                        return Err(RuntimeError::ProtocolInvariant {
                            detail: "公司批准缺少完整 live 组候选".into(),
                        });
                    }
                    for (index, group) in candidate.groups.iter().enumerate() {
                        let valid = group.group as usize == index
                            && group.accounts.is_empty()
                            && group.information.as_ref().is_some_and(|prepared| {
                                prepared.library == expected.library
                                    && prepared.current_facts == expected.current_facts
                                    && prepared.personals.is_empty()
                                    && prepared.observed_through.is_none()
                            });
                        if !valid {
                            return Err(RuntimeError::ProtocolInvariant {
                                detail: format!(
                                    "公司批准组候选与无资产、无观察合同不符：group={}",
                                    group.group
                                ),
                            });
                        }
                    }
                    if !inspect() {
                        return Err(RuntimeError::ProtocolInvariant {
                            detail: "公司批准准备期间 Session revision 已变化".into(),
                        });
                    }
                    Ok(())
                });
        result.map_err(|error| CompanyApprovalFailure {
            error: Box::new(error),
            terminal: self.world.is_terminal(),
        })
    }
}
