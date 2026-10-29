//! 成员加入先准备完整新 authority，再登记 receiver，最后只移动已验证值。
use super::*;
use crate::runtime::drive::RuntimeDriver;

#[cfg(test)]
thread_local! { static STOP_FAILURE_FOR_TEST: std::cell::Cell<u8> = const { std::cell::Cell::new(0) }; }

#[cfg(test)]
pub(in crate::session) fn fail_next_stop_for_test(prepared: bool) {
    STOP_FAILURE_FOR_TEST.with(|slot| {
        assert_eq!(slot.get(), 0);
        slot.set(if prepared { 1 } else { 2 });
    });
}

struct PreparedMemberHandover {
    state: CommittableSessionState,
    driver: RuntimeDriver,
    mirror: PreparedRuntimeMirror,
    ack: crate::runtime::source::PreparedMirrorAck,
    epoch: u64,
}

impl GameSession {
    fn add_member_to_private_state(&mut self, subject: OpaqueSubjectId, member: &MarketMembership) {
        let account = member.account_id;
        self.state.accounts.insert(
            account,
            Account::new(
                account,
                AccountKind::Player,
                member.admission_funding.external_cash,
            ),
        );
        self.state.history_reads.insert(
            account,
            crate::experience::PersonalHistoryReadLedger::default(),
        );
        self.state
            .memberships
            .members
            .insert(subject, member.clone());
    }

    pub(in crate::session) fn join_with_runtime_handover(
        &mut self,
        subject: OpaqueSubjectId,
        member: MarketMembership,
    ) -> Result<MarketMembership, MembershipError> {
        let failure = |description| StepFatal::InvariantViolation {
            location: "GameSession::join_market::handover".to_owned(),
            description,
        };
        self.external_revision().checked_add(1).ok_or_else(|| {
            MembershipError::RuntimeHandover("external revision 已耗尽，拒绝成员加入".to_owned())
        })?;
        if self.has_current_external_runtime_authority() {
            return Err(MembershipError::RuntimeHandover(
                "外部 RuntimeDriver 独占当前 authority，不能通过内部成员入口替换".to_owned(),
            ));
        }
        // 尚无 World 时没有运行态需要移交；所有私有账户值也在登记 receiver 前准备。
        if self.runtime_driver.is_none() {
            let mut candidate = self
                .clone_for_runtime_handover()
                .map_err(|fatal| MembershipError::RuntimeHandover(fatal.to_string()))?;
            candidate.add_member_to_private_state(subject, &member);
            if let Some(binding) = &self.ingress {
                binding
                    .source
                    .register_player(member.account_id)
                    .map_err(|error| MembershipError::IngressRegistration(error.to_string()))?;
            }
            self.state = candidate.state;
            self.mark_external_mutation();
            return Ok(member);
        }
        let epoch = self
            .next_runtime_authority_epoch()
            .map_err(MembershipError::RuntimeHandover)?;
        self.runtime_mirror_install_sequence
            .checked_add(1)
            .ok_or_else(|| {
                MembershipError::RuntimeHandover("install_sequence 已耗尽，拒绝成员加入".to_owned())
            })?;
        let cell = self
            .runtime_driver
            .take()
            .expect("上方已确认原 driver 在位");
        let preparation = (|| {
            let mut old = cell.lock().map_err(|_| {
                let fatal = self.poison_runtime_step(failure("原 driver 锁 poison".to_owned()));
                MembershipError::RuntimeHandover(fatal.to_string())
            })?;
            old.ensure_session_authority(self)
                .map_err(MembershipError::RuntimeHandover)?;
            if old.world_committed_tick() != self.tick() {
                return Err(MembershipError::RuntimeHandover(
                    "移交前 World 与 Session progress 不一致".to_owned(),
                ));
            }
            let captured = old.capture_runtime_handover().map_err(|description| {
                let fatal = self.poison_runtime_step(failure(description));
                MembershipError::RuntimeHandover(fatal.to_string())
            })?;
            let input = old
                .prepare_flush_driver_projection(self)
                .map_err(|description| {
                    let fatal = self.poison_runtime_step(failure(description));
                    MembershipError::RuntimeHandover(fatal.to_string())
                })?;
            let mirror = old
                .prepare_flush_candidate(
                    self,
                    input,
                    crate::runtime::sync_policy::FlushTrigger::OnDemand,
                )
                .map_err(MembershipError::RuntimeHandover)?;
            self.validate_runtime_mirror_source(&old, &mirror)
                .map_err(|fatal| MembershipError::RuntimeHandover(fatal.to_string()))?;
            if captured.information.source != old.mirror_source() {
                return Err(MembershipError::RuntimeHandover(
                    "完整移交与静点镜像来源不一致".to_owned(),
                ));
            }
            let mut candidate = self
                .clone_for_runtime_handover()
                .map_err(|fatal| MembershipError::RuntimeHandover(fatal.to_string()))?;
            candidate.materialize_runtime_handover_base(mirror);
            candidate.add_member_to_private_state(subject, &member);
            RuntimeDriver::validate_static_adoption_identity(&candidate)
                .map_err(MembershipError::RuntimeHandover)?;
            let mut driver = RuntimeDriver::prepare_from_session_grouped(
                &candidate,
                self.runtime_group_count,
                epoch,
            )
            .map_err(|error| {
                let description = error.to_string();
                if error.cleanup_failed {
                    self.poison_runtime_step(failure(description.clone()));
                }
                MembershipError::RuntimeHandover(description)
            })?;
            let values = (|| {
                driver.seed_prepared_runtime_handover(&captured)?;
                driver.adopt_session_seq(&candidate);
                let input = driver.prepare_static_driver_projection(&candidate)?;
                let mirror = candidate.prepare_runtime_handover_mirror(&mut driver, input)?;
                let ack = self
                    .validate_runtime_mirror_source(&driver, &mirror)
                    .map_err(|fatal| fatal.to_string())?;
                Ok::<_, String>((mirror, ack))
            })();
            let (mirror, ack) = match values {
                Ok(value) => value,
                Err(error) => {
                    return match driver.stop_prepared_without_drain() {
                        Ok(()) => Err(MembershipError::RuntimeHandover(error)),
                        Err(cleanup) => {
                            let fatal = self.poison_runtime_step(failure(format!(
                                "{error}；prepared worker 回收失败：{cleanup}"
                            )));
                            Err(MembershipError::RuntimeHandover(fatal.to_string()))
                        }
                    }
                }
            };
            Ok(PreparedMemberHandover {
                state: candidate.state,
                driver,
                mirror,
                ack,
                epoch,
            })
        })();
        self.runtime_driver = Some(cell);
        let prepared = preparation?;
        #[cfg(test)]
        let stop_failure = STOP_FAILURE_FOR_TEST.with(|slot| slot.replace(0));
        #[cfg(test)]
        if stop_failure == 1 {
            prepared.driver.inject_handover_stop_failure_for_test();
        }
        // receiver 注册是最后一个可失败业务步骤；之后不再创建线程、读 getter 或投影。
        let registration = self
            .ingress
            .as_ref()
            .map(|binding| binding.source.register_player(member.account_id))
            .transpose();
        if let Err(error) = registration {
            return match prepared.driver.stop_prepared_without_drain() {
                Ok(()) => Err(MembershipError::IngressRegistration(error.to_string())),
                Err(cleanup) => {
                    let fatal = self.poison_runtime_step(failure(format!(
                        "receiver 注册失败：{error}；prepared worker 回收失败：{cleanup}"
                    )));
                    Err(MembershipError::RuntimeHandover(fatal.to_string()))
                }
            };
        }
        let old = self
            .runtime_driver
            .take()
            .expect("原 authority 在登记前一直保留");
        self.state = prepared.state;
        self.install_runtime_authority_epoch(prepared.epoch);
        let mut driver = prepared.driver;
        let events = self.install_runtime_mirror(&mut driver, prepared.mirror);
        assert!(events.is_empty(), "成员移交不制造公开市场事实");
        driver.ack_prepared_mirror(prepared.ack);
        self.runtime_driver = Some(std::sync::Mutex::new(driver));
        self.state.runtime_information_authority = true;
        let (old, lock_poisoned) = match old.into_inner() {
            Ok(driver) => (driver, false),
            Err(poison) => (poison.into_inner(), true),
        };
        #[cfg(test)]
        if stop_failure == 2 {
            old.inject_handover_stop_failure_for_test();
        }
        let stopped = old.stop_prepared_without_drain();
        if lock_poisoned || stopped.is_err() {
            let description = match stopped {
                Ok(()) => "旧 driver 锁 poison；已回收全部旧 worker".to_owned(),
                Err(error) => format!("旧 driver 退役失败，锁 poison={lock_poisoned}：{error}"),
            };
            let fatal = self.poison_runtime_step(failure(description));
            return Err(MembershipError::RuntimeHandover(fatal.to_string()));
        }
        Ok(member)
    }
}
