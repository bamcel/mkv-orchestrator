use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self)
            }
        }
    };
}

uuid_id!(PlanId);
uuid_id!(JobId);
uuid_id!(CorrelationId);
uuid_id!(RenameBatchId);
uuid_id!(MediaServerId);

/// Caller-provided key that makes a mutation safe to retry.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    pub fn parse(value: impl Into<String>) -> Result<Self, IdempotencyKeyError> {
        let value = value.into().trim().to_owned();
        if value.is_empty() {
            return Err(IdempotencyKeyError::Empty);
        }
        if value.len() > 200 {
            return Err(IdempotencyKeyError::TooLong);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn generate() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for IdempotencyKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum IdempotencyKeyError {
    #[error("idempotency key must not be empty")]
    Empty,
    #[error("idempotency key must not exceed 200 characters")]
    TooLong,
}
