//! posix_spawn client for `snowbox-eval`. This crate must not link
//! nix-bindings / libgc (ADR 0019).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cache::Cache;
use crate::sandbox::ActionError;

pub struct Realized {
    pub out_path: String,
    pub export: Vec<u8>,
}

/// JSON-line protocol with snowbox-eval. Keep fields in sync with
/// `eval/src/protocol.rs`.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    EvalString {
        expr: String,
        origin: String,
    },
    Realize {
        flake_dir: PathBuf,
        work_dir: PathBuf,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct Response {
    ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    out_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    export_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    nars: Vec<NarFile>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct NarFile {
    store_path: String,
    nar_path: PathBuf,
    #[serde(default)]
    references: Vec<String>,
}

struct WorkDir(PathBuf);

impl WorkDir {
    fn new() -> Result<Self, ActionError> {
        let dir = std::env::temp_dir().join(format!("snowbox-eval-{}", Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("nars"))
            .map_err(|e| ActionError::Failed(e.to_string()))?;
        Ok(Self(dir))
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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

pub fn realize_environment(flake_dir: &Path, cache: &Cache) -> Result<Realized, ActionError> {
    let work = WorkDir::new()?;
    let resp = call(&Request::Realize {
        flake_dir: flake_dir.to_path_buf(),
        work_dir: work.0.clone(),
    })?;
    for nar in &resp.nars {
        let bytes = std::fs::read(&nar.nar_path).map_err(|e| {
            ActionError::Failed(format!("read NAR {}: {e}", nar.nar_path.display()))
        })?;
        cache
            .put_nar(&nar.store_path, &bytes, &nar.references)
            .map_err(|e| ActionError::Failed(e.to_string()))?;
    }
    let export_path = resp
        .export_path
        .clone()
        .unwrap_or_else(|| work.0.join("export"));
    let export = std::fs::read(&export_path)
        .map_err(|e| ActionError::Failed(format!("read export {}: {e}", export_path.display())))?;
    let out_path = resp
        .out_path
        .ok_or_else(|| ActionError::Failed("snowbox-eval returned no out_path".into()))?;
    Ok(Realized { out_path, export })
}

fn eval_bin() -> Result<PathBuf, ActionError> {
    if let Some(p) = std::env::var_os("SNOWBOX_EVAL") {
        return Ok(PathBuf::from(p));
    }
    let exe = std::env::current_exe().map_err(|e| ActionError::Failed(e.to_string()))?;
    let candidate = exe
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("snowbox-eval");
    if candidate.is_file() {
        return Ok(candidate);
    }
    Err(ActionError::Failed(format!(
        "snowbox-eval not found (set SNOWBOX_EVAL or cargo build -p snowbox-eval); looked at {}",
        candidate.display()
    )))
}

/// `std::process::Command` on macOS is posix_spawn. Do not libc::fork.
fn call(req: &Request) -> Result<Response, ActionError> {
    call_bin(&eval_bin()?, req)
}

fn call_bin(bin: &Path, req: &Request) -> Result<Response, ActionError> {
    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| ActionError::Failed(format!("spawn snowbox-eval ({}): {e}", bin.display())))?;
    {
        let mut stdin = child.stdin.take().ok_or(ActionError::Internal)?;
        let mut line = serde_json::to_vec(req).map_err(|_| ActionError::Internal)?;
        line.push(b'\n');
        stdin
            .write_all(&line)
            .map_err(|e| ActionError::Failed(e.to_string()))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| ActionError::Failed(e.to_string()))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.lines().next().unwrap_or("");
    let resp: Response = serde_json::from_str(line).map_err(|_| {
        ActionError::Failed(format!(
            "snowbox-eval: bad stdout (status {:?}): {line}",
            out.status.code()
        ))
    })?;
    if !resp.ok {
        return Err(ActionError::Failed(
            resp.error.unwrap_or_else(|| "snowbox-eval failed".into()),
        ));
    }
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_flake_url_encodes_spaces() {
        let p = Path::new("/Users/me/Application Support/snowbox/environment");
        assert_eq!(
            path_flake_url(p),
            "path:/Users/me/Application%20Support/snowbox/environment"
        );
    }

    #[test]
    fn request_is_one_json_line() {
        let req = Request::EvalString {
            expr: "\"hi\"".into(),
            origin: "<t>".into(),
        };
        let line = serde_json::to_string(&req).unwrap();
        assert!(!line.contains('\n'));
        assert_eq!(
            line,
            r#"{"op":"eval_string","expr":"\"hi\"","origin":"<t>"}"#
        );
        let back: Request = serde_json::from_str(&line).unwrap();
        match back {
            Request::EvalString { expr, origin } => {
                assert_eq!(expr, "\"hi\"");
                assert_eq!(origin, "<t>");
            }
            Request::Realize { .. } => panic!("wrong op"),
        }
    }

    #[test]
    fn realize_request_and_response_roundtrip() {
        let req = Request::Realize {
            flake_dir: "/tmp/env".into(),
            work_dir: "/tmp/work".into(),
        };
        let line = serde_json::to_string(&req).unwrap();
        assert!(!line.contains('\n'));
        let back: Request = serde_json::from_str(&line).unwrap();
        match back {
            Request::Realize {
                flake_dir,
                work_dir,
            } => {
                assert_eq!(flake_dir, PathBuf::from("/tmp/env"));
                assert_eq!(work_dir, PathBuf::from("/tmp/work"));
            }
            Request::EvalString { .. } => panic!("wrong op"),
        }

        let resp = Response {
            ok: true,
            out_path: Some("/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-x".into()),
            export_path: Some("/tmp/work/export".into()),
            nars: vec![NarFile {
                store_path: "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-x".into(),
                nar_path: "/tmp/work/nars/0.nar".into(),
                references: vec![],
            }],
            ..Response::default()
        };
        let decoded: Response =
            serde_json::from_str(&serde_json::to_string(&resp).unwrap()).unwrap();
        assert!(decoded.ok);
        assert_eq!(
            decoded.nars[0].nar_path,
            PathBuf::from("/tmp/work/nars/0.nar")
        );
    }

    #[test]
    fn missing_helper_is_failed() {
        let err = call_bin(
            Path::new("/no/such/snowbox-eval"),
            &Request::EvalString {
                expr: "\"x\"".into(),
                origin: "<t>".into(),
            },
        )
        .unwrap_err();
        match err {
            ActionError::Failed(msg) => {
                assert!(msg.contains("spawn snowbox-eval"), "{msg}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }
}
