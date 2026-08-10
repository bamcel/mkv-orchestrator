use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Cheap file identity used to detect stale cache records and stale plans.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileFingerprint {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub modified_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quick_hash: Option<String>,
}

impl FileFingerprint {
    #[must_use]
    pub fn matches(&self, other: &Self) -> bool {
        self.path == other.path
            && self.size_bytes == other.size_bytes
            && self.modified_at == other.modified_at
            && match (&self.quick_hash, &other.quick_hash) {
                (Some(left), Some(right)) => left == right,
                _ => true,
            }
    }
}

/// Tool identity captured in a preview and rechecked before apply.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolFingerprint {
    pub name: String,
    pub executable: PathBuf,
    pub version: String,
}

/// Hash a serializable value using stable struct and `BTreeMap` field order.
pub fn stable_fingerprint<T: Serialize>(value: &T) -> Result<String, FingerprintError> {
    let bytes = serde_json::to_vec(value).map_err(FingerprintError::Serialize)?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

#[derive(Debug, thiserror::Error)]
pub enum FingerprintError {
    #[error("value could not be fingerprinted: {0}")]
    Serialize(serde_json::Error),
}

/// Configuration values that affect a plan without copying secrets into it.
pub type SettingsFingerprint = String;

/// Tool fingerprints keyed by logical tool name.
pub type ToolFingerprints = BTreeMap<String, ToolFingerprint>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_hash_is_independent_of_map_insertion_order() {
        let mut one = BTreeMap::new();
        one.insert("z", 2);
        one.insert("a", 1);
        let mut two = BTreeMap::new();
        two.insert("a", 1);
        two.insert("z", 2);
        assert_eq!(
            stable_fingerprint(&one).unwrap(),
            stable_fingerprint(&two).unwrap()
        );
    }
}
