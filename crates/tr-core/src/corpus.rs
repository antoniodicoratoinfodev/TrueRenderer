//! Temporary R0 input gate shared by broker and decoder. Remove only after OS qualification.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

#[derive(Deserialize)]
struct Manifest {
    images: Vec<Entry>,
}
#[derive(Deserialize)]
struct Entry {
    sha256: String,
}
pub struct CorpusPolicy {
    approved: HashSet<String>,
}
impl Default for CorpusPolicy {
    fn default() -> Self {
        let manifest: Manifest =
            serde_json::from_str(include_str!("../../../corpus/manifest.json"))
                .expect("Trusted built-in corpus manifest");
        Self {
            approved: manifest.images.into_iter().map(|i| i.sha256).collect(),
        }
    }
}
impl CorpusPolicy {
    pub fn approves(&self, digest: &str) -> bool {
        self.approved.contains(digest)
    }
    pub fn approves_bytes(&self, bytes: &[u8]) -> bool {
        self.approves(&format!("{:x}", Sha256::digest(bytes)))
    }
}
