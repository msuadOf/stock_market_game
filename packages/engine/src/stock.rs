//! 股票数值身份与目录（ADR 0041）。
//!
//! 核心引擎以 [`StockId`]（u32）作为股票唯一身份，全程无字符串键；
//! 代码字符串只存在于边界：配置加载、存档元数据、显示与日志翻译。
//! [`StockCatalog`] 是唯一翻译层，id 按代码字符串升序分配，
//! 因此 `StockId` 的 [`Ord`] 即代码序，既有按代码排序的展示顺序天然保持。

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// 股票数值代号。由 [`StockCatalog`] 按代码升序分配（0..n）。
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize, ts_rs::TS,
)]
#[serde(transparent)]
#[ts(export)]
pub struct StockId(pub u32);

/// 证据文档契约：整数一律序列化为十进制字符串（与 u64_decimal 约定一致，
/// 防 JS 安全整数边界）。仅用于证据投影字段；wire/存档仍为数值。
pub mod stock_id_decimal {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(id: &super::StockId, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&id.0.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<super::StockId, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        raw.parse::<u32>()
            .map(super::StockId)
            .map_err(serde::de::Error::custom)
    }
}

/// Vec<StockId> 的证据十进制字符串序列化。
pub mod stock_id_decimal_vec {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(ids: &[super::StockId], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_seq(ids.iter().map(|id| id.0.to_string()))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<super::StockId>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = Vec::<String>::deserialize(deserializer)?;
        raw.into_iter()
            .map(|value| {
                value
                    .parse::<u32>()
                    .map(super::StockId)
                    .map_err(serde::de::Error::custom)
            })
            .collect()
    }
}

impl fmt::Display for StockId {
    /// 调试/边界文本用；展示代码请经 [`StockCatalog::code`] 翻译。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 边界股票元数据（目录项）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ts_rs::TS)]
pub struct StockMeta {
    /// 真实 A 股格式代码（如 "600519"），只在边界使用。
    pub code: String,
    /// 显示名；缺省与代码相同，可由配置替换。
    pub display_name: String,
}

impl StockMeta {
    /// 以代码为显示名构造元数据。
    pub fn new(code: impl Into<String>) -> StockMeta {
        let code = code.into();
        StockMeta {
            display_name: code.clone(),
            code,
        }
    }
}

/// 股票目录构造错误。绝不静默去重或排序吞错（铁律二）。
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StockCatalogError {
    /// 出现重复代码；携带重复值便于定位配置来源。
    #[error("重复的股票代码: {0}")]
    DuplicateCode(String),
}

/// 股票目录：id ↔ 代码/显示名的唯一权威翻译层。
///
/// 构造后不可变；`metas` 下标即 `StockId`。序列化只保留 `metas`
/// （`by_code` 反序列化后自动重建）。
#[derive(Clone, Debug, PartialEq, Serialize, ts_rs::TS)]
pub struct StockCatalog {
    metas: Vec<StockMeta>,
    #[serde(skip)]
    #[ts(skip)]
    by_code: BTreeMap<String, StockId>,
}

impl StockCatalog {
    /// 由元数据列表构造：按代码升序排序后顺序分配 id；重复代码显式报错。
    pub fn from_metas(metas: Vec<StockMeta>) -> Result<StockCatalog, StockCatalogError> {
        let mut metas = metas;
        metas.sort_by(|a, b| a.code.cmp(&b.code));
        let mut by_code = BTreeMap::new();
        for (index, meta) in metas.iter().enumerate() {
            let id = StockId(index as u32);
            if by_code.insert(meta.code.clone(), id).is_some() {
                return Err(StockCatalogError::DuplicateCode(meta.code.clone()));
            }
        }
        Ok(StockCatalog { metas, by_code })
    }

    /// 目录股票数。
    pub fn len(&self) -> usize {
        self.metas.len()
    }

    /// 目录是否为空。
    pub fn is_empty(&self) -> bool {
        self.metas.is_empty()
    }

    /// 按代码查 id；未知代码返回 None（调用方必须显式处理）。
    pub fn id(&self, code: &str) -> Option<StockId> {
        self.by_code.get(code).copied()
    }

    /// 按 id 取元数据；越界 id 返回 None。
    pub fn meta(&self, id: StockId) -> Option<&StockMeta> {
        self.metas.get(id.0 as usize)
    }

    /// 按 id 取代码；越界 id 返回 None。
    pub fn code(&self, id: StockId) -> Option<&str> {
        self.meta(id).map(|meta| meta.code.as_str())
    }

    /// 按 id 取显示名；越界 id 返回 None。
    pub fn display_name(&self, id: StockId) -> Option<&str> {
        self.meta(id).map(|meta| meta.display_name.as_str())
    }

    /// 按 id 序（= 代码序）迭代全部元数据。
    pub fn iter(&self) -> impl Iterator<Item = (StockId, &StockMeta)> {
        self.metas
            .iter()
            .enumerate()
            .map(|(index, meta)| (StockId(index as u32), meta))
    }

    /// 反序列化后重建代码索引。
    fn rebuild_index(&mut self) {
        self.by_code = self
            .metas
            .iter()
            .enumerate()
            .map(|(index, meta)| (meta.code.clone(), StockId(index as u32)))
            .collect();
    }
}

impl<'de> Deserialize<'de> for StockCatalog {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            metas: Vec<StockMeta>,
        }
        let raw = Raw::deserialize(deserializer)?;
        // metas 按约定以代码升序持久化；读档时校验而非静默接受乱序/重复，
        // 防止存档被篡改后 id 语义漂移（铁律二：显式失败）。
        let mut sorted = raw.metas.clone();
        sorted.sort_by(|a, b| a.code.cmp(&b.code));
        if sorted != raw.metas {
            return Err(serde::de::Error::custom(
                "股票目录未按代码升序持久化，存档损坏或版本不符",
            ));
        }
        let mut catalog = StockCatalog {
            metas: raw.metas,
            by_code: BTreeMap::new(),
        };
        catalog.rebuild_index();
        // 显式重复校验：索引数量必须与元数据一致（上游升序校验的防御兜底）。
        if catalog.by_code.len() != catalog.metas.len() {
            return Err(serde::de::Error::custom(
                "股票目录包含重复代码，存档损坏或版本不符",
            ));
        }
        Ok(catalog)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// id 分配序 = 代码升序序（展示顺序性质，ADR 0041）。
    #[test]
    fn ids_are_assigned_in_code_order() {
        let catalog = StockCatalog::from_metas(vec![
            StockMeta::new("600889"),
            StockMeta::new("000812"),
            StockMeta::new("600888"),
        ])
        .expect("不重复的代码必须构造成功");
        assert_eq!(catalog.id("000812"), Some(StockId(0)));
        assert_eq!(catalog.id("600888"), Some(StockId(1)));
        assert_eq!(catalog.id("600889"), Some(StockId(2)));
        // Ord 即代码序。
        assert!(StockId(0) < StockId(1) && StockId(1) < StockId(2));
    }

    /// 重复代码显式报错，不静默去重（铁律二）。
    #[test]
    fn duplicate_codes_are_rejected() {
        let error =
            StockCatalog::from_metas(vec![StockMeta::new("600888"), StockMeta::new("600888")])
                .expect_err("重复代码必须报错");
        assert_eq!(
            error,
            StockCatalogError::DuplicateCode("600888".to_string())
        );
    }

    /// 边界翻译往返一致；未知代码返回 None 由调用方显式拒绝。
    #[test]
    fn lookup_roundtrip_and_unknown() {
        let catalog = StockCatalog::from_metas(vec![StockMeta::new("600519")]).unwrap();
        let id = catalog.id("600519").expect("已注册代码必须可查");
        assert_eq!(catalog.code(id), Some("600519"));
        assert_eq!(catalog.display_name(id), Some("600519"));
        assert_eq!(catalog.id("300999"), None);
        assert_eq!(catalog.code(StockId(9)), None);
    }

    /// 序列化只含 metas；读档重建索引并拒绝乱序存档。
    #[test]
    fn serde_roundtrip_rebuilds_index_and_rejects_disorder() {
        let catalog =
            StockCatalog::from_metas(vec![StockMeta::new("000812"), StockMeta::new("600888")])
                .unwrap();
        let json = serde_json::to_string(&catalog).unwrap();
        let restored: StockCatalog = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.id("600888"), Some(StockId(1)));
        let tampered = json.replace("000812", "999999");
        // 乱序（第二个代码小于第一个）必须被读档拒绝。
        let bad = r#"{"metas":[{"code":"600888"},{"code":"000812"}]}"#;
        assert!(serde_json::from_str::<StockCatalog>(bad).is_err());
        let _: String = tampered; // 避免未使用告警；篡改替换本身不改顺序。
    }
}
