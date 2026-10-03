//! 按显式接触时刻选择未受保护条目；各容器仍独立持有自己的时钟和事实。

use std::collections::BTreeSet;

use crate::StockCode;

pub(super) struct RetentionCandidates<'a> {
    contacts: Vec<(u64, &'a StockCode)>,
    protected: &'a BTreeSet<StockCode>,
    cap: usize,
}

impl<'a> RetentionCandidates<'a> {
    pub(super) fn from_contacts(
        contacts: impl Iterator<Item = (&'a StockCode, u64)>,
        protected: &'a BTreeSet<StockCode>,
        cap: usize,
    ) -> Self {
        Self {
            contacts: contacts.map(|(code, minute)| (minute, code)).collect(),
            protected,
            cap,
        }
    }

    pub(super) fn select_kept_unprotected(self) -> BTreeSet<StockCode> {
        let mut unprotected: Vec<_> = self
            .contacts
            .into_iter()
            .filter(|(_, code)| !self.protected.contains(*code))
            .collect();
        unprotected.sort_by(|left, right| right.cmp(left));
        unprotected
            .into_iter()
            .take(self.cap)
            .map(|(_, code)| code.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_caps_only_unprotected_contacts_and_breaks_ties_by_code() {
        let contacts: Vec<_> = (0..12)
            .map(|index| (StockCode(format!("600{index:03}")), 7))
            .collect();
        let protected = contacts[..3].iter().map(|(code, _)| code.clone()).collect();
        let kept = RetentionCandidates::from_contacts(
            contacts.iter().map(|(code, minute)| (code, *minute)),
            &protected,
            8,
        )
        .select_kept_unprotected();
        assert_eq!(kept.len(), 8);
        assert!(!kept.contains(&contacts[3].0));
        assert!(kept.contains(&contacts[4].0));
        assert!(!kept.iter().any(|code| protected.contains(code)));
    }
}
