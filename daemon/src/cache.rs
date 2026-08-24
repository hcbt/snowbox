//! Snowbox Cache: a Host `file://` binary cache the Daemon owns.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::sandbox::ActionError;

#[derive(Clone, Debug)]
pub struct Cache {
    root: PathBuf,
}

impl Cache {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, ActionError> {
        let root = root.into();
        std::fs::create_dir_all(&root).map_err(|_| ActionError::Internal)?;
        std::fs::create_dir_all(root.join("nar")).map_err(|_| ActionError::Internal)?;
        std::fs::write(
            root.join("nix-cache-info"),
            "StoreDir: /nix/store\nWantMassQuery: 1\nPriority: 40\n",
        )
        .map_err(|_| ActionError::Internal)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn substituter_uri(&self) -> String {
        format!("file://{}", self.root.display())
    }

    pub fn put_nar(
        &self,
        store_path: &str,
        nar: &[u8],
        references: &[String],
    ) -> std::io::Result<()> {
        let digest = Sha256::digest(nar);
        let nar_hash = nixbase32(&digest);
        let url = format!("nar/{nar_hash}.nar");
        std::fs::write(self.root.join("nar").join(format!("{nar_hash}.nar")), nar)?;

        let hash = store_hash(store_path);
        let refs = references
            .iter()
            .map(|r| r.rsplit('/').next().unwrap_or(r.as_str()))
            .collect::<Vec<_>>()
            .join(" ");
        let info = format!(
            "StorePath: {store_path}\n\
             URL: {url}\n\
             Compression: none\n\
             FileHash: sha256:{nar_hash}\n\
             FileSize: {}\n\
             NarHash: sha256:{nar_hash}\n\
             NarSize: {}\n\
             References: {refs}\n",
            nar.len(),
            nar.len()
        );
        std::fs::write(self.root.join(format!("{hash}.narinfo")), info)?;
        Ok(())
    }
}

fn store_hash(store_path: &str) -> &str {
    let name = store_path.rsplit('/').next().unwrap_or(store_path);
    name.split_once('-').map(|(h, _)| h).unwrap_or(name)
}

fn nixbase32(hash: &[u8]) -> String {
    const CHARS: &[u8] = b"0123456789abcdfghijklmnpqrsvwxyz";
    let n = hash.len();
    let len2 = (n * 8 - 1) / 5 + 1;
    let mut out = String::with_capacity(len2);
    for i in (0..len2).rev() {
        let b = i * 5;
        let i_byte = b / 8;
        let j = b % 8;
        let v1 = (hash[i_byte] as usize) >> j;
        let v2 = if i_byte >= n - 1 {
            0
        } else {
            (hash[i_byte + 1] as usize) << (8 - j)
        };
        out.push(CHARS[(v1 | v2) & 0x1f] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_file_cache_layout() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(dir.path().join("cache")).unwrap();
        assert!(cache.root().join("nar").is_dir());
        assert!(cache.substituter_uri().starts_with("file://"));
        let info = std::fs::read_to_string(cache.root().join("nix-cache-info")).unwrap();
        assert!(info.contains("StoreDir: /nix/store"));
        assert!(info.contains("WantMassQuery: 1"));
        assert!(info.contains("Priority: 40"));
    }

    #[test]
    fn put_nar_writes_narinfo_and_hashed_nar() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(dir.path().join("cache")).unwrap();
        let nar = b"nix-archive-1-not-real";
        cache
            .put_nar(
                "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-hello",
                nar,
                &["/nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-glibc".into()],
            )
            .unwrap();
        let info = std::fs::read_to_string(
            cache
                .root()
                .join("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.narinfo"),
        )
        .unwrap();
        assert!(info.contains("StorePath: /nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-hello"));
        assert!(info.contains("Compression: none"));
        assert!(info.contains("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-glibc"));
        let url = info
            .lines()
            .find(|l| l.starts_with("URL:"))
            .unwrap()
            .trim_start_matches("URL:")
            .trim();
        assert!(url.starts_with("nar/"));
        assert!(url.ends_with(".nar"));
        assert_eq!(std::fs::read(cache.root().join(url)).unwrap(), nar);
        assert!(info.contains("FileHash: sha256:"));
        assert!(info.contains("NarHash: sha256:"));
    }

    #[test]
    fn nixbase32_sha256_hello() {
        let digest = Sha256::digest(b"hello");
        assert_eq!(
            format!("{digest:x}"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        // `nix hash file --base32 --type sha256` of the bytes `hello`
        assert_eq!(
            nixbase32(&digest),
            "094qif9n4cq4fdg459qzbhg1c6wywawwaaivx0k0x8xhbyx4vwic"
        );
    }
}
