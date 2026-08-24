//! JSON-line protocol with the Daemon client. Keep fields in sync with
//! `daemon/src/nix.rs`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
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
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nars: Vec<NarFile>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NarFile {
    pub store_path: String,
    pub nar_path: PathBuf,
    #[serde(default)]
    pub references: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_string_is_one_json_line() {
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
            _ => panic!("wrong op"),
        }
    }

    #[test]
    fn realize_response_roundtrip() {
        let resp = Response {
            ok: true,
            out_path: Some("/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-x".into()),
            export_path: Some("/tmp/export".into()),
            nars: vec![NarFile {
                store_path: "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-x".into(),
                nar_path: "/tmp/nars/0.nar".into(),
                references: vec!["/nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-y".into()],
            }],
            ..Response::default()
        };
        let line = serde_json::to_string(&resp).unwrap();
        assert!(!line.contains('\n'));
        let back: Response = serde_json::from_str(&line).unwrap();
        assert!(back.ok);
        assert_eq!(back.nars.len(), 1);
        assert_eq!(back.nars[0].nar_path, PathBuf::from("/tmp/nars/0.nar"));
    }

    #[test]
    fn error_response_roundtrip() {
        let resp = Response {
            ok: false,
            error: Some("eval: boom".into()),
            ..Response::default()
        };
        let back: Response = serde_json::from_str(&serde_json::to_string(&resp).unwrap()).unwrap();
        assert!(!back.ok);
        assert_eq!(back.error.as_deref(), Some("eval: boom"));
    }
}
