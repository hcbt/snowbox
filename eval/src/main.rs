//! One-shot Nix C API helper. JSON line on stdin, JSON line on stdout.
//! NAR bytes go to a file named in the JSON, not across the pipe.

mod nar;
mod protocol;
mod realize;

use std::io::{self, BufRead, Write};

use protocol::{Request, Response};

fn main() {
    match run() {
        Ok(()) => {}
        Err(error) => {
            let resp = Response {
                ok: false,
                error: Some(error),
                ..Response::default()
            };
            let _ = writeln!(
                io::stdout(),
                "{}",
                serde_json::to_string(&resp)
                    .unwrap_or_else(|_| { r#"{"ok":false,"error":"encode failed"}"#.to_string() })
            );
            std::process::exit(1);
        }
    }
}

fn run() -> Result<(), String> {
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    let req: Request = serde_json::from_str(line.trim()).map_err(|e| format!("request: {e}"))?;
    let resp = match req {
        Request::EvalString { expr, origin } => realize::eval_string(&expr, &origin)?,
        Request::Realize {
            flake_dir,
            work_dir,
        } => realize::realize(&flake_dir, &work_dir)?,
    };
    let mut out = serde_json::to_string(&resp).map_err(|e| e.to_string())?;
    out.push('\n');
    io::stdout()
        .write_all(out.as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(())
}
