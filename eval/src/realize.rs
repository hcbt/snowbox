//! Eval and realise through nix-bindings. nixops4 has no copy_closure.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use nix_bindings_expr::eval_state::{self, EvalStateBuilder, gc_register_my_thread};
use nix_bindings_flake::{EvalStateBuilderExt, FlakeSettings};
use nix_bindings_store::store::Store as NixStore;

use crate::nar;
use crate::protocol::{NarFile, Response};

/// Guest Environments are Linux. Darwin Hosts realize aarch64-linux
/// through linux-builder.
const PREFERRED_SYSTEM: &str = "aarch64-linux";

pub fn eval_string(expr: &str, origin: &str) -> Result<Response, String> {
    with_eval(|es| {
        let value = es
            .eval_from_string(expr, origin)
            .map_err(|e| format!("eval: {e}"))?;
        let s = es
            .require_string(&value)
            .map_err(|e| format!("eval string: {e}"))?;
        Ok(Response {
            ok: true,
            value: Some(s),
            ..Response::default()
        })
    })
}

pub fn realize(flake_dir: &Path, work_dir: &Path) -> Result<Response, String> {
    with_eval(|es| realize_with(es, flake_dir, work_dir))
}

fn realize_with(
    es: &mut nix_bindings_expr::eval_state::EvalState,
    flake_dir: &Path,
    work_dir: &Path,
) -> Result<Response, String> {
    let flake_path = flake_dir.canonicalize().map_err(|e| e.to_string())?;
    let flake_url = path_flake_url(&flake_path);
    let expr = format!(
        "let f = builtins.getFlake ''{flake_url}''; \
         preferred = ''{PREFERRED_SYSTEM}''; \
         names = builtins.attrNames (f.packages or {{}}); \
         sys = if builtins.elem preferred names then preferred \
               else if names == [] then preferred \
               else builtins.head names; \
         in \"${{f.packages.${{sys}}.default}}\""
    );
    let value = es
        .eval_from_string(&expr, "<snowbox-environment>")
        .map_err(|e| format!("eval environment: {e}"))?;
    let realised = es
        .realise_string(&value, false)
        .map_err(|e| format!("realise environment: {e}"))?;
    let out_path = realised.s.trim().to_string();
    if !out_path.starts_with("/nix/store/") {
        return Err(format!(
            "environment did not realise to a store path: {out_path}"
        ));
    }

    let mut store = es.store().clone();
    let root = store
        .parse_store_path(&out_path)
        .map_err(|e| e.to_string())?;
    let closure = store
        .get_fs_closure(&root, false, false, false)
        .map_err(|e| e.to_string())?;

    let mut refs: HashMap<String, Vec<String>> = HashMap::new();
    for p in &closure {
        let rp = store.real_path(p).map_err(|e| e.to_string())?;
        let close = store
            .get_fs_closure(p, false, false, false)
            .map_err(|e| e.to_string())?;
        let mut r = Vec::new();
        for c in close {
            let cr = store.real_path(&c).map_err(|e| e.to_string())?;
            if cr != rp {
                r.push(cr);
            }
        }
        refs.insert(rp, r);
    }

    let order = topo(&refs)?;
    fs::create_dir_all(work_dir).map_err(|e| e.to_string())?;
    let nars_dir = work_dir.join("nars");
    fs::create_dir_all(&nars_dir).map_err(|e| e.to_string())?;
    let export_path = work_dir.join("export");
    let mut export = fs::File::create(&export_path).map_err(|e| e.to_string())?;

    let mut nars = Vec::new();
    for (i, path) in order.iter().enumerate() {
        let nar_path = nars_dir.join(format!("{i}.nar"));
        {
            let mut f = fs::File::create(&nar_path).map_err(|e| e.to_string())?;
            nar::dump_path_to(&PathBuf::from(path), &mut f).map_err(|e| e.to_string())?;
        }
        let empty = Vec::new();
        let path_refs = refs.get(path).unwrap_or(&empty);
        {
            let mut nar_f = fs::File::open(&nar_path).map_err(|e| e.to_string())?;
            nar::write_export_path(&mut export, path, &mut nar_f, path_refs)
                .map_err(|e| e.to_string())?;
        }
        nars.push(NarFile {
            store_path: path.clone(),
            nar_path,
            references: path_refs.clone(),
        });
    }
    nar::write_export_end(&mut export).map_err(|e| e.to_string())?;

    Ok(Response {
        ok: true,
        out_path: Some(out_path),
        export_path: Some(export_path),
        nars,
        ..Response::default()
    })
}

fn with_eval<T>(
    f: impl FnOnce(&mut nix_bindings_expr::eval_state::EvalState) -> Result<T, String>,
) -> Result<T, String> {
    eval_state::init().map_err(|e| e.to_string())?;
    let _gc = gc_register_my_thread().map_err(|e| e.to_string())?;
    let _ = nix_bindings_util::settings::set("experimental-features", "nix-command flakes");
    let store = NixStore::open(None, []).map_err(|e| e.to_string())?;
    let flake_settings = FlakeSettings::new().map_err(|e| e.to_string())?;
    let mut es = EvalStateBuilder::new(store)
        .map_err(|e| e.to_string())?
        .flakes(&flake_settings)
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?;
    f(&mut es)
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

fn topo(refs: &HashMap<String, Vec<String>>) -> Result<Vec<String>, String> {
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
