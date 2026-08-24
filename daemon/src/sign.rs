//! Ad-hoc codesign so Virtualization.framework will start a guest.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SignDecision {
    Proceed,
    SignAndReexec,
    FailAlreadyTried,
}

/// `SNOWBOX_SIGNED=1` means re-exec already ran. It is not a skip of the
/// entitlement check.
pub(crate) fn sign_decision(has_entitlement: bool, already_reexeced: bool) -> SignDecision {
    if has_entitlement {
        SignDecision::Proceed
    } else if already_reexeced {
        SignDecision::FailAlreadyTried
    } else {
        SignDecision::SignAndReexec
    }
}

#[cfg(target_os = "macos")]
pub fn ensure_signed() {
    let already_reexeced = std::env::var("SNOWBOX_SIGNED").as_deref() == Ok("1");
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(exe_str) = exe.to_str() else {
        return;
    };
    let has_entitlement = has_virtualization_entitlement(exe_str);
    match sign_decision(has_entitlement, already_reexeced) {
        SignDecision::Proceed => {}
        SignDecision::FailAlreadyTried => {
            eprintln!(
                "snowbox is not signed with com.apple.security.virtualization (SNOWBOX_SIGNED=1)"
            );
            std::process::exit(1);
        }
        SignDecision::SignAndReexec => {
            eprintln!("signing {exe_str} with com.apple.security.virtualization");
            if let Err(e) = sign_binary(exe_str) {
                eprintln!("codesign failed: {e}");
                std::process::exit(1);
            }
            use std::os::unix::process::CommandExt;
            let err = std::process::Command::new(&exe)
                .args(std::env::args_os().skip(1))
                .env("SNOWBOX_SIGNED", "1")
                .exec();
            eprintln!("re-exec after signing failed: {err}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn ensure_signed() {}

#[cfg(target_os = "macos")]
fn has_virtualization_entitlement(exe: &str) -> bool {
    let output = std::process::Command::new("codesign")
        .args(["-d", "--entitlements", "-", "--xml", exe])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).contains("com.apple.security.virtualization")
        }
        _ => false,
    }
}

#[cfg(target_os = "macos")]
fn sign_binary(exe: &str) -> Result<(), String> {
    let entitlements = include_str!("../macos/virtualization.entitlements");
    let path = std::env::temp_dir().join("snowbox-virtualization.entitlements");
    std::fs::write(&path, entitlements).map_err(|e| e.to_string())?;
    let output = std::process::Command::new("codesign")
        .args(["--sign", "-", "--force", "--entitlements"])
        .arg(&path)
        .arg(exe)
        .output()
        .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&path);
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entitlements_plist_declares_virtualization() {
        assert!(
            include_str!("../macos/virtualization.entitlements")
                .contains("com.apple.security.virtualization")
        );
    }

    #[test]
    fn signed_env_without_entitlement_is_not_a_skip() {
        assert_eq!(sign_decision(false, true), SignDecision::FailAlreadyTried);
        assert_eq!(sign_decision(true, true), SignDecision::Proceed);
        assert_eq!(sign_decision(true, false), SignDecision::Proceed);
        assert_eq!(sign_decision(false, false), SignDecision::SignAndReexec);
    }
}
