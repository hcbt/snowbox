//! nixpkgs catalog. The GUI searches by program name and description;
//! the Environment still stores the attribute name.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::nix;
use crate::sandbox::ActionError;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    pub program: String,
    pub description: String,
    pub unfree: bool,
}

pub struct Catalog {
    flake_dir: Option<PathBuf>,
    cache_file: Option<PathBuf>,
    names: Mutex<Vec<String>>,
    meta: Mutex<std::collections::HashMap<String, Package>>,
}

impl Catalog {
    pub fn memory(entries: Vec<Package>) -> Self {
        let mut names = Vec::new();
        let mut meta = std::collections::HashMap::new();
        for e in entries {
            names.push(e.name.clone());
            meta.insert(e.name.clone(), e);
        }
        names.sort();
        Self {
            flake_dir: None,
            cache_file: None,
            names: Mutex::new(names),
            meta: Mutex::new(meta),
        }
    }

    pub fn from_flake(dir: impl Into<PathBuf>, cache_file: impl Into<PathBuf>) -> Self {
        Self {
            flake_dir: Some(dir.into()),
            cache_file: Some(cache_file.into()),
            names: Mutex::new(Vec::new()),
            meta: Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub fn search(
        &self,
        q: &str,
        allow_unfree: bool,
        limit: usize,
    ) -> Result<Vec<Package>, ActionError> {
        let q = q.trim().to_lowercase();
        if q.is_empty() {
            return Err(ActionError::BadRequest("query required"));
        }
        self.ensure_index()?;
        let meta = self.meta.lock().map_err(|_| ActionError::Internal)?;
        let mut out: Vec<Package> = meta
            .values()
            .filter(|p| allow_unfree || !p.unfree)
            .filter(|p| matches_query(p, &q))
            .cloned()
            .collect();
        out.sort_by(|a, b| {
            rank(&a.program, &q)
                .cmp(&rank(&b.program, &q))
                .then(rank(&a.name, &q).cmp(&rank(&b.name, &q)))
                .then(a.name.cmp(&b.name))
        });
        out.truncate(limit);
        Ok(out)
    }

    fn ensure_index(&self) -> Result<(), ActionError> {
        {
            let meta = self.meta.lock().map_err(|_| ActionError::Internal)?;
            if !meta.is_empty() {
                return Ok(());
            }
        }
        if let Some(cache) = &self.cache_file {
            if cache.is_file() {
                if let Ok(entries) = load_cache(cache) {
                    self.install(entries)?;
                    return Ok(());
                }
            }
        }
        let Some(dir) = &self.flake_dir else {
            return Ok(());
        };
        let entries = load_all(dir)?;
        if let Some(cache) = &self.cache_file {
            let _ = std::fs::write(cache, serde_json::to_vec(&entries).unwrap_or_default());
        }
        self.install(entries)
    }

    fn install(&self, entries: Vec<Package>) -> Result<(), ActionError> {
        let mut meta = self.meta.lock().map_err(|_| ActionError::Internal)?;
        let mut stored = self.names.lock().map_err(|_| ActionError::Internal)?;
        if meta.is_empty() {
            stored.clear();
            for p in entries {
                stored.push(p.name.clone());
                meta.insert(p.name.clone(), p);
            }
            stored.sort();
        }
        Ok(())
    }
}

fn matches_query(p: &Package, q: &str) -> bool {
    p.name.to_lowercase().contains(q)
        || p.program.to_lowercase().contains(q)
        || p.description.to_lowercase().contains(q)
}

fn rank(s: &str, q: &str) -> u8 {
    let s = s.to_lowercase();
    if s == q {
        0
    } else if s.starts_with(q) {
        1
    } else if s.contains(q) {
        2
    } else {
        3
    }
}

fn guest_system() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "aarch64-linux"
    } else {
        "x86_64-linux"
    }
}

fn load_cache(path: &Path) -> Result<Vec<Package>, ActionError> {
    let raw = std::fs::read_to_string(path).map_err(|_| ActionError::Internal)?;
    serde_json::from_str(&raw).map_err(|_| ActionError::Internal)
}

fn load_all(flake_dir: &Path) -> Result<Vec<Package>, ActionError> {
    let url = nix::path_flake_url_pub(flake_dir)?;
    let sys = guest_system();
    let expr = format!(
        r#"
        let
          pkgs = (builtins.getFlake ''{url}'').inputs.nixpkgs.legacyPackages.{sys};
          lib = pkgs.lib;
          go = name: pkg:
            let
              p = builtins.tryEval pkg;
            in if !p.success then null
            else if !(p.value ? type && p.value.type == "derivation") then null
            else let
              m = builtins.tryEval (p.value.meta or {{}});
              meta = if m.success then m.value else {{}};
            in {{
              inherit name;
              program = meta.mainProgram or p.value.pname or name;
              description = meta.description or "";
              unfree = meta.unfree or false;
            }};
        in builtins.toJSON (builtins.filter (x: x != null) (lib.mapAttrsToList go pkgs))
        "#
    );
    let raw = nix::eval_string(&expr, "<snowbox-catalog>")?;
    serde_json::from_str(&raw).map_err(|_| ActionError::Internal)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat() -> Catalog {
        Catalog::memory(vec![
            Package {
                name: "hello".into(),
                program: "hello".into(),
                description: "A program that produces a familiar, friendly greeting".into(),
                unfree: false,
            },
            Package {
                name: "jq".into(),
                program: "jq".into(),
                description: "Lightweight and flexible command-line JSON processor".into(),
                unfree: false,
            },
            Package {
                name: "ripgrep".into(),
                program: "rg".into(),
                description: "Utility that combines the usability of The Silver Searcher with the speed of grep".into(),
                unfree: false,
            },
            Package {
                name: "unrar".into(),
                program: "unrar".into(),
                description: "Utility for RAR archives".into(),
                unfree: true,
            },
        ])
    }

    #[test]
    fn searches_by_program_name() {
        let hits = cat().search("rg", false, 10).unwrap();
        assert!(
            hits.iter()
                .any(|p| p.program == "rg" && p.name == "ripgrep")
        );
        assert!(!hits.iter().any(|p| p.name == "unrar"));
    }

    #[test]
    fn searches_by_description() {
        let hits = cat().search("json", false, 10).unwrap();
        assert!(hits.iter().any(|p| p.name == "jq"));
    }

    #[test]
    fn unfree_opt_in() {
        assert!(cat().search("unrar", false, 10).unwrap().is_empty());
        let hits = cat().search("unrar", true, 10).unwrap();
        assert!(hits.iter().any(|p| p.unfree && p.name == "unrar"));
    }

    #[test]
    fn empty_query_is_bad() {
        assert!(matches!(
            cat().search("  ", false, 10),
            Err(ActionError::BadRequest(_))
        ));
    }
}
