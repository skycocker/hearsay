use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use ulid::Ulid;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(Ulid);

impl SessionId {
    pub fn new() -> Self {
        Self(Ulid::new())
    }

    pub fn as_ulid(&self) -> Ulid {
        self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for SessionId {
    type Err = ulid::DecodeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ulid::from_str(s).map(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_through_string() {
        let id = SessionId::new();
        let s = id.to_string();
        let parsed: SessionId = s.parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn serde_json_roundtrip() {
        let id = SessionId::new();
        let j = serde_json::to_string(&id).unwrap();
        let back: SessionId = serde_json::from_str(&j).unwrap();
        assert_eq!(id, back);
    }
}
