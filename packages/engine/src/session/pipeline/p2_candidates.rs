use crate::{AccountId, Intent};
use std::cmp::Ordering;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CandidateSource {
    Npc,
    Player,
    PlanChain,
}

impl CandidateSource {
    const fn rank(self) -> u8 {
        match self {
            Self::Npc => 0,
            Self::Player => 1,
            Self::PlanChain => 2,
        }
    }
}

impl Ord for CandidateSource {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank().cmp(&other.rank())
    }
}

impl PartialOrd for CandidateSource {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CandidateSourceLocalKey {
    Npc {
        account: AccountId,
        npc_local_index: u64,
    },
    Player {
        player_queue_index: u64,
    },
    PlanChain {
        chain_generation_index: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum P2CandidateKey {
    Npc {
        account: AccountId,
        npc_local_index: u64,
    },
    Player {
        player_queue_index: u64,
    },
    PlanChain {
        chain_generation_index: u64,
    },
}

impl P2CandidateKey {
    pub const fn npc(account: AccountId, npc_local_index: u64) -> Self {
        Self::Npc {
            account,
            npc_local_index,
        }
    }

    pub const fn player(player_queue_index: u64) -> Self {
        Self::Player { player_queue_index }
    }

    pub const fn plan_chain(chain_generation_index: u64) -> Self {
        Self::PlanChain {
            chain_generation_index,
        }
    }

    pub const fn source(&self) -> CandidateSource {
        match self {
            Self::Npc { .. } => CandidateSource::Npc,
            Self::Player { .. } => CandidateSource::Player,
            Self::PlanChain { .. } => CandidateSource::PlanChain,
        }
    }

    pub const fn source_local_key(&self) -> CandidateSourceLocalKey {
        match self {
            Self::Npc {
                account,
                npc_local_index,
            } => CandidateSourceLocalKey::Npc {
                account: *account,
                npc_local_index: *npc_local_index,
            },
            Self::Player { player_queue_index } => CandidateSourceLocalKey::Player {
                player_queue_index: *player_queue_index,
            },
            Self::PlanChain {
                chain_generation_index,
            } => CandidateSourceLocalKey::PlanChain {
                chain_generation_index: *chain_generation_index,
            },
        }
    }
}

impl Ord for P2CandidateKey {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (
                Self::Npc {
                    account: left_account,
                    npc_local_index: left_index,
                },
                Self::Npc {
                    account: right_account,
                    npc_local_index: right_index,
                },
            ) => (left_account, left_index).cmp(&(right_account, right_index)),
            (
                Self::Player {
                    player_queue_index: left,
                },
                Self::Player {
                    player_queue_index: right,
                },
            ) => left.cmp(right),
            (
                Self::PlanChain {
                    chain_generation_index: left,
                },
                Self::PlanChain {
                    chain_generation_index: right,
                },
            ) => left.cmp(right),
            _ => self.source().cmp(&other.source()),
        }
    }
}

impl PartialOrd for P2CandidateKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum P2CandidateError {
    DuplicateKey(P2CandidateKey),
    InvalidSourceSequence,
}

impl std::fmt::Display for P2CandidateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateKey(key) => write!(formatter, "duplicate P2 candidate key {key:?}"),
            Self::InvalidSourceSequence => formatter.write_str("P2 source sequence is invalid"),
        }
    }
}

impl std::error::Error for P2CandidateError {}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct P2Candidate {
    key: P2CandidateKey,
    owner: AccountId,
    intent: Intent,
}

impl P2Candidate {
    pub fn new(key: P2CandidateKey, owner: AccountId, intent: Intent) -> Self {
        Self { key, owner, intent }
    }

    pub const fn key(&self) -> &P2CandidateKey {
        &self.key
    }

    pub const fn owner(&self) -> AccountId {
        self.owner
    }

    pub const fn intent(&self) -> &Intent {
        &self.intent
    }
}

impl PartialEq for P2Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
            && self.owner == other.owner
            && match (&self.intent, &other.intent) {
                (
                    Intent::PlaceLimit {
                        code: left_code,
                        side: left_side,
                        price: left_price,
                        qty: left_qty,
                    },
                    Intent::PlaceLimit {
                        code: right_code,
                        side: right_side,
                        price: right_price,
                        qty: right_qty,
                    },
                ) => {
                    left_code == right_code
                        && left_side == right_side
                        && left_price == right_price
                        && left_qty == right_qty
                }
                (
                    Intent::PlaceMarket {
                        code: left_code,
                        side: left_side,
                        qty: left_qty,
                    },
                    Intent::PlaceMarket {
                        code: right_code,
                        side: right_side,
                        qty: right_qty,
                    },
                ) => left_code == right_code && left_side == right_side && left_qty == right_qty,
                (
                    Intent::Cancel {
                        code: left_code,
                        id: left_id,
                    },
                    Intent::Cancel {
                        code: right_code,
                        id: right_id,
                    },
                ) => left_code == right_code && left_id == right_id,
                _ => false,
            }
    }
}

impl Eq for P2Candidate {}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct P2CandidateBatch {
    candidates: Vec<P2Candidate>,
}

impl<'de> serde::Deserialize<'de> for P2CandidateBatch {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct Transfer {
            candidates: Vec<P2Candidate>,
        }

        let transfer = Transfer::deserialize(deserializer)?;
        Self::new(transfer.candidates).map_err(serde::de::Error::custom)
    }
}

impl P2CandidateBatch {
    /// Preserve the source's actual candidate order. Keys identify requests; sorting by key
    /// would turn source class and account number into an unrequested trading priority.
    pub fn new(candidates: Vec<P2Candidate>) -> Result<Self, P2CandidateError> {
        let mut seen = BTreeSet::new();
        for candidate in &candidates {
            if !seen.insert(candidate.key.clone()) {
                return Err(P2CandidateError::DuplicateKey(candidate.key.clone()));
            }
        }
        Ok(Self { candidates })
    }

    pub fn candidates(&self) -> &[P2Candidate] {
        &self.candidates
    }
}
