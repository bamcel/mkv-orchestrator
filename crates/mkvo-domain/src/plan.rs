use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    CONTRACT_VERSION, FileFingerprint, FingerprintError, IdempotencyKey, PlanId, ToolFingerprints,
    stable_fingerprint,
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Scan,
    Rename,
    RenameUndo,
    Remux,
    ConvertToMkv,
    MuxSubtitles,
    ExtractSubtitles,
    PropertyEdit,
    LibraryAudit,
    CacheReconcile,
    MediaServerSync,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceAccess {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanConflictKind {
    MissingSource,
    MissingParent,
    ExistingTarget,
    DuplicateTarget,
    UnauthorizedPath,
    ReadOnly,
    Busy,
    UnsupportedInput,
    InvalidSelection,
    EmptyOutput,
    NoChange,
    StaleInput,
    ToolUnavailable,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanConflict {
    pub kind: PlanConflictKind,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub blocking: bool,
}

impl PlanConflict {
    #[must_use]
    pub fn blocking(
        kind: PlanConflictKind,
        message: impl Into<String>,
        path: Option<PathBuf>,
    ) -> Self {
        Self {
            kind,
            message: message.into(),
            path,
            blocking: true,
        }
    }

    #[must_use]
    pub fn informational(
        kind: PlanConflictKind,
        message: impl Into<String>,
        path: Option<PathBuf>,
    ) -> Self {
        Self {
            kind,
            message: message.into(),
            path,
            blocking: false,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceClaim {
    pub path: PathBuf,
    pub access: ResourceAccess,
}

impl ResourceClaim {
    #[must_use]
    pub fn read(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            access: ResourceAccess::Read,
        }
    }

    #[must_use]
    pub fn write(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            access: ResourceAccess::Write,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanContext {
    pub settings_fingerprint: String,
    #[serde(default)]
    pub tool_fingerprints: ToolFingerprints,
    #[serde(default)]
    pub input_fingerprints: Vec<FileFingerprint>,
    #[serde(default)]
    pub resources: Vec<ResourceClaim>,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

impl PlanContext {
    #[must_use]
    pub fn empty(settings_fingerprint: impl Into<String>) -> Self {
        Self {
            settings_fingerprint: settings_fingerprint.into(),
            tool_fingerprints: BTreeMap::new(),
            input_fingerprints: Vec::new(),
            resources: Vec::new(),
            attributes: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanMetadata {
    pub id: PlanId,
    pub contract_version: u32,
    pub kind: OperationKind,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub idempotency_key: IdempotencyKey,
    pub request_fingerprint: String,
    pub fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationPlan<T> {
    pub metadata: PlanMetadata,
    pub context: PlanContext,
    pub payload: T,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FingerprintSeed<'a, T> {
    id: PlanId,
    contract_version: u32,
    kind: OperationKind,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    idempotency_key: &'a IdempotencyKey,
    request_fingerprint: &'a str,
    context: &'a PlanContext,
    payload: &'a T,
}

impl<T: Serialize> OperationPlan<T> {
    pub fn new(
        kind: OperationKind,
        request: &impl Serialize,
        payload: T,
        context: PlanContext,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        idempotency_key: IdempotencyKey,
    ) -> Result<Self, PlanBuildError> {
        if expires_at <= created_at {
            return Err(PlanBuildError::InvalidExpiry);
        }
        let request_fingerprint = stable_fingerprint(request)?;
        let id = PlanId::new();
        let seed = FingerprintSeed {
            id,
            contract_version: CONTRACT_VERSION,
            kind,
            created_at,
            expires_at,
            idempotency_key: &idempotency_key,
            request_fingerprint: &request_fingerprint,
            context: &context,
            payload: &payload,
        };
        let fingerprint = stable_fingerprint(&seed)?;
        Ok(Self {
            metadata: PlanMetadata {
                id,
                contract_version: CONTRACT_VERSION,
                kind,
                created_at,
                expires_at,
                idempotency_key,
                request_fingerprint,
                fingerprint,
            },
            context,
            payload,
        })
    }

    pub fn validate_integrity(&self, now: DateTime<Utc>) -> Result<(), PlanValidationError> {
        if self.metadata.contract_version != CONTRACT_VERSION {
            return Err(PlanValidationError::UnsupportedContractVersion {
                expected: CONTRACT_VERSION,
                actual: self.metadata.contract_version,
            });
        }
        if now > self.metadata.expires_at {
            return Err(PlanValidationError::Expired(self.metadata.expires_at));
        }
        let seed = FingerprintSeed {
            id: self.metadata.id,
            contract_version: self.metadata.contract_version,
            kind: self.metadata.kind,
            created_at: self.metadata.created_at,
            expires_at: self.metadata.expires_at,
            idempotency_key: &self.metadata.idempotency_key,
            request_fingerprint: &self.metadata.request_fingerprint,
            context: &self.context,
            payload: &self.payload,
        };
        let actual = stable_fingerprint(&seed).map_err(PlanValidationError::Fingerprint)?;
        if actual != self.metadata.fingerprint {
            return Err(PlanValidationError::FingerprintMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PlanBuildError {
    #[error("plan expiration must be after creation")]
    InvalidExpiry,
    #[error(transparent)]
    Fingerprint(#[from] FingerprintError),
}

#[derive(Debug, thiserror::Error)]
pub enum PlanValidationError {
    #[error("plan expired at {0}")]
    Expired(DateTime<Utc>),
    #[error("plan fingerprint does not match its payload")]
    FingerprintMismatch,
    #[error("unsupported contract version: expected {expected}, got {actual}")]
    UnsupportedContractVersion { expected: u32, actual: u32 },
    #[error(transparent)]
    Fingerprint(FingerprintError),
}

/// Persistence-neutral representation used by plan repositories.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredPlan {
    pub metadata: PlanMetadata,
    pub context: PlanContext,
    pub payload: serde_json::Value,
}

impl<T: Serialize> TryFrom<&OperationPlan<T>> for StoredPlan {
    type Error = serde_json::Error;

    fn try_from(plan: &OperationPlan<T>) -> Result<Self, Self::Error> {
        Ok(Self {
            metadata: plan.metadata.clone(),
            context: plan.context.clone(),
            payload: serde_json::to_value(&plan.payload)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    #[test]
    fn changing_payload_invalidates_plan() {
        let now = Utc::now();
        let mut plan = OperationPlan::new(
            OperationKind::Rename,
            &"request",
            vec!["target"],
            PlanContext::empty("settings"),
            now,
            now + Duration::minutes(5),
            IdempotencyKey::generate(),
        )
        .unwrap();
        assert!(plan.validate_integrity(now).is_ok());
        plan.payload.push("tampered");
        assert!(matches!(
            plan.validate_integrity(now),
            Err(PlanValidationError::FingerprintMismatch)
        ));
    }

    #[test]
    fn rejects_expired_plan() {
        let now = Utc::now();
        let plan = OperationPlan::new(
            OperationKind::Remux,
            &"request",
            (),
            PlanContext::empty("settings"),
            now,
            now + Duration::seconds(1),
            IdempotencyKey::generate(),
        )
        .unwrap();
        assert!(matches!(
            plan.validate_integrity(now + Duration::seconds(2)),
            Err(PlanValidationError::Expired(_))
        ));
    }
}
