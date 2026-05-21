use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AgentId(u64);

impl AgentId {
    pub fn from_transcript_path(path: &str) -> Self {
        let mut hash: u64 = 0xcbf29ce484222325;
        for b in path.as_bytes() {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        AgentId(hash)
    }

    pub fn raw(self) -> u64 {
        self.0
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_id_is_deterministic_per_path() {
        let a = AgentId::from_transcript_path("/Users/me/.claude/projects/x/abc.jsonl");
        let b = AgentId::from_transcript_path("/Users/me/.claude/projects/x/abc.jsonl");
        assert_eq!(a, b);
    }

    #[test]
    fn agent_id_differs_per_path() {
        let a = AgentId::from_transcript_path("/Users/me/.claude/projects/x/abc.jsonl");
        let b = AgentId::from_transcript_path("/Users/me/.claude/projects/x/def.jsonl");
        assert_ne!(a, b);
    }

    #[test]
    fn agent_id_displays_as_hex() {
        let id = AgentId::from_transcript_path("x");
        assert_eq!(format!("{id}").len(), 16);
    }
}
