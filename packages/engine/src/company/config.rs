use super::simple::SimpleConfig;

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(tag = "mode", content = "config", deny_unknown_fields)]
pub enum CompanySystemConfig {
    Simple(SimpleConfig),
    Simulation,
}
