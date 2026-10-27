//! Simple 模型的局内偏好配置接缝：只提交公司系统及 setup 的一致配置候选。

use super::{GameSession, StepFatal};
use crate::company::config::CompanySystemConfig;
use crate::company::simple::SimpleCompanyPreferences;
use crate::company::{CompanyId, CompanySystem, CompanySystemError};

/// 仅供 Rust 调用的偏好错误；保留健康状态及公司域的原始原因，不定义 wire 契约。
#[derive(Debug, thiserror::Error)]
pub enum SimplePreferencesError {
    #[error(transparent)]
    Step(#[from] StepFatal),
    #[error(transparent)]
    Company(#[from] CompanySystemError),
}

impl GameSession {
    /// 读取公司 Simple 偏好；不发起公司行为、不改变个人信息或外部版本。
    pub fn company_simple_preferences(
        &self,
        company: &CompanyId,
    ) -> Result<SimpleCompanyPreferences, SimplePreferencesError> {
        self.require_healthy()?;
        let (system, _) = self.simple_preferences_configuration(company)?;
        Ok(system.simple_preferences(company)?)
    }

    /// 局内编辑只影响后续偏好评估；既有方案与提案台账保持原样。
    pub fn set_simple_preferences(
        &mut self,
        company: &CompanyId,
        preferences: SimpleCompanyPreferences,
    ) -> Result<(), SimplePreferencesError> {
        self.require_healthy()?;
        let (system, config) = self.simple_preferences_configuration(company)?;
        let mut candidate_system = system.clone();
        let mut candidate_config = config.clone();
        candidate_system.set_simple_preferences(company, preferences.clone())?;
        let CompanySystemConfig::Simple(simple) = &mut candidate_config else {
            return Err(CompanySystemError::Unsupported(format!(
                "公司 {} 的局内偏好编辑仅支持 Simple",
                company.0
            ))
            .into());
        };
        let entry = simple
            .companies
            .iter_mut()
            .find(|entry| &entry.company == company)
            .ok_or_else(|| CompanySystemError::InvalidInput(format!("未知公司 {}", company.0)))?;
        entry.preferences = preferences;
        simple.validate_for_issuers(candidate_system.issuers())?;
        if candidate_system.config() != candidate_config {
            return Err(CompanySystemError::SystemState(format!(
                "公司 {} 的 Simple 偏好候选与 setup 配置不一致",
                company.0
            ))
            .into());
        }
        // 所有可失败操作完成后才标记并同时安装；不触碰常驻 runtime driver。
        self.mark_external_mutation();
        *self
            .state
            .companies
            .system_arc_mut()
            .expect("配置校验已确认 Simple 公司状态") = std::sync::Arc::new(candidate_system);
        self.state.setup.company_system = Some(candidate_config);
        Ok(())
    }

    fn simple_preferences_configuration(
        &self,
        company: &CompanyId,
    ) -> Result<(&CompanySystem, &CompanySystemConfig), CompanySystemError> {
        match (
            &self.state.companies.system_arc(),
            &self.state.setup.company_system,
        ) {
            (None, None) => Err(CompanySystemError::Unsupported(format!(
                "本局未启用公司系统，无法访问公司 {} 的 Simple 偏好",
                company.0
            ))),
            (Some(system), Some(config @ CompanySystemConfig::Simple(_))) => {
                if system.config() != *config {
                    return Err(CompanySystemError::SystemState(format!(
                        "公司 {} 的公司系统配置与 setup 不一致",
                        company.0
                    )));
                }
                Ok((system, config))
            }
            (Some(_), Some(CompanySystemConfig::Simulation)) => {
                Err(CompanySystemError::Unsupported(format!(
                    "公司 {} 的局内偏好配置仅支持 Simple",
                    company.0
                )))
            }
            _ => Err(CompanySystemError::SystemState(format!(
                "公司 {} 的公司系统与 setup 在场性不一致",
                company.0
            ))),
        }
    }
}
