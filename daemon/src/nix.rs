//! Realize an Environment flake through nix-bindings. Copy is NAR dump
//! (nixops4 has no copy_closure).

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use nix_bindings_expr::eval_state::{self, EvalStateBuilder, gc_register_my_thread};
use nix_bindings_flake::{EvalStateBuilderExt, FlakeSettings};
use nix_bindings_store::store::Store as NixStore;

use crate::cache::Cache;
use crate::nar;
use crate::sandbox::ActionError;

pub struct Realized {
    pub out_path: String,
    pub export: Vec<u8>,
}

fn path_flake_url(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let mut enc = String::from("path:");
    for c in raw.chars() {
        match c {
            ' ' => enc.push_str("%20"),
            '%' => enc.push_str("%25"),
            '?' => enc.push_str("%3F"),
            '#' => enc.push_str("%23"),
            _ => enc.push(c),
        }
    }
    enc
}

static REALIZE: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn realize_environment(flake_dir: &Path, cache: &Cache) -> Result<Realized, ActionError> {
    let _realize = REALIZE.lock().map_err(|_| ActionError::Internal)?;
    eval_state::init().map_err(|e| ActionError::Failed(e.to_string()))?;
    let _gc = gc_register_my_thread().map_err(|e| ActionError::Failed(e.to_string()))?;
    let _ = nix_bindings_util::settings::set("experimental-features", "nix-command flakes");

    let store = NixStore::open(None, []).map_err(|e| ActionError::Failed(e.to_string()))?;
    let flake_settings = FlakeSettings::new().map_err(|e| ActionError::Failed(e.to_string()))?;
    let mut es = EvalStateBuilder::new(store.clone())
        .map_err(|e| ActionError::Failed(e.to_string()))?
        .flakes(&flake_settings)
        .map_err(|e| ActionError::Failed(e.to_string()))?
        .build()
        .map_err(|e| ActionError::Failed(e.to_string()))?;

    let flake_path = flake_dir
        .canonicalize()
        .map_err(|e| ActionError::Failed(e.to_string()))?;
    let flake_url = path_flake_url(&flake_path);
    let expr =
        format!("\"${{(builtins.getFlake ''{flake_url}'').packages.aarch64-linux.default}}\"");
    let value = es
        .eval_from_string(&expr, "<snowbox-environment>")
        .map_err(|e| ActionError::Failed(format!("eval environment: {e}")))?;
    let realised = es
        .realise_string(&value, false)
        .map_err(|e| ActionError::Failed(format!("realise environment: {e}")))?;
    let out_path = realised.s.trim().to_string();
    if !out_path.starts_with("/nix/store/") {
        return Err(ActionError::Failed(format!(
            "environment did not realise to a store path: {out_path}"
        )));
    }

    let mut store = es.store().clone();
    let root = store
        .parse_store_path(&out_path)
        .map_err(|e| ActionError::Failed(e.to_string()))?;
    let closure = store
        .get_fs_closure(&root, false, false, false)
        .map_err(|e| ActionError::Failed(e.to_string()))?;

    let mut refs: HashMap<String, Vec<String>> = HashMap::new();
    for p in &closure {
        let rp = store
            .real_path(p)
            .map_err(|e| ActionError::Failed(e.to_string()))?;
        let close = store
            .get_fs_closure(p, false, false, false)
            .map_err(|e| ActionError::Failed(e.to_string()))?;
        let mut r = Vec::new();
        for c in close {
            let cr = store
                .real_path(&c)
                .map_err(|e| ActionError::Failed(e.to_string()))?;
            if cr != rp {
                r.push(cr);
            }
        }
        refs.insert(rp, r);
    }

    let order = topo(&refs)?;
    let mut export = Vec::new();
    for path in &order {
        let disk = PathBuf::from(path);
        let nar_bytes = nar::dump_path(&disk).map_err(|e| ActionError::Failed(e.to_string()))?;
        cache
            .put_nar(path, &nar_bytes)
            .map_err(|e| ActionError::Failed(e.to_string()))?;
        let framed = nar::export_path(
            path,
            &nar_bytes,
            refs.get(path).map(|v| v.as_slice()).unwrap_or(&[]),
        )
        .map_err(|e| ActionError::Failed(e.to_string()))?;
        export.extend_from_slice(&framed);
    }
    export.extend_from_slice(&nar::export_end());
    Ok(Realized { out_path, export })
}

fn topo(refs: &HashMap<String, Vec<String>>) -> Result<Vec<String>, ActionError> {
    // path depends on its references: refs must come first
    let mut indeg: HashMap<String, usize> = HashMap::new();
    let mut rev: HashMap<String, Vec<String>> = HashMap::new();
    for (p, rs) in refs {
        indeg.entry(p.clone()).or_insert(0);
        for r in rs {
            indeg.entry(r.clone()).or_insert(0);
            *indeg.entry(p.clone()).or_insert(0) += 1;
            rev.entry(r.clone()).or_default().push(p.clone());
        }
    }
    let mut q: VecDeque<String> = indeg
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(k, _)| k.clone())
        .collect();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    while let Some(n) = q.pop_front() {
        if !seen.insert(n.clone()) {
            continue;
        }
        out.push(n.clone());
        if let Some(children) = rev.get(&n) {
            for c in children {
                if let Some(d) = indeg.get_mut(c) {
                    *d = d.saturating_sub(1);
                    if *d == 0 {
                        q.push_back(c.clone());
                    }
                }
            }
        }
    }
    if out.len() != refs.len() {
        // cycle or missing: emit remaining in arbitrary order
        for k in refs.keys() {
            if !seen.contains(k) {
                out.push(k.clone());
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topo_puts_deps_first() {
        let mut refs = HashMap::new();
        refs.insert("a".into(), vec!["b".into()]);
        refs.insert("b".into(), vec![]);
        let order = topo(&refs).unwrap();
        let b = order.iter().position(|x| x == "b").unwrap();
        let a = order.iter().position(|x| x == "a").unwrap();
        assert!(b < a);
    }

    #[test]
    fn path_flake_url_encodes_spaces() {
        let p = Path::new("/Users/me/Application Support/snowbox/environment");
        assert_eq!(
            path_flake_url(p),
            "path:/Users/me/Application%20Support/snowbox/environment"
        );
    }
}
