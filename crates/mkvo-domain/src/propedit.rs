use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{FileFingerprint, OperationPlan, PlanConflict, TrackKind};

pub type PropertyEditPlan = OperationPlan<PropertyEditPlanPayload>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropertyEditPlanPayload {
    #[serde(default)]
    pub items: Vec<PropertyEditPlanItem>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropertyEditPlanItem {
    pub path: PathBuf,
    pub source_fingerprint: FileFingerprint,
    #[serde(default)]
    pub mutations: Vec<PropertyMutation>,
    #[serde(default)]
    pub conflicts: Vec<PlanConflict>,
}

impl PropertyEditPlanItem {
    #[must_use]
    pub fn can_apply(&self) -> bool {
        !self.mutations.is_empty() && !self.conflicts.iter().any(|conflict| conflict.blocking)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PropertyMutation {
    SetContainerTitle {
        value: String,
    },
    DeleteContainerTitle,
    SetTrackName {
        selector: TrackSelector,
        value: String,
    },
    DeleteTrackName {
        selector: TrackSelector,
    },
    SetTrackLanguage {
        selector: TrackSelector,
        language: String,
    },
    SetDefaultFlag {
        selector: TrackSelector,
        value: bool,
    },
    SetForcedFlag {
        selector: TrackSelector,
        value: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackSelector {
    pub kind: TrackKind,
    /// One-based ordinal within the track kind, as required by mkvpropedit.
    pub ordinal: u32,
}

impl TrackSelector {
    pub fn new(kind: TrackKind, ordinal: u32) -> Result<Self, TrackSelectorError> {
        if ordinal == 0 {
            return Err(TrackSelectorError::ZeroOrdinal);
        }
        if matches!(kind, TrackKind::Buttons | TrackKind::Other) {
            return Err(TrackSelectorError::UnsupportedKind(kind));
        }
        Ok(Self { kind, ordinal })
    }

    #[must_use]
    pub fn mkvpropedit_value(self) -> String {
        format!("track:{}{}", self.kind.propedit_prefix(), self.ordinal)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum TrackSelectorError {
    #[error("track ordinal must be one-based")]
    ZeroOrdinal,
    #[error("track kind {0:?} cannot be selected by mkvpropedit")]
    UnsupportedKind(TrackKind),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_uses_type_specific_one_based_ordinal() {
        let selector = TrackSelector::new(TrackKind::Subtitle, 2).unwrap();
        assert_eq!(selector.mkvpropedit_value(), "track:s2");
    }
}
