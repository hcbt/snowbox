//! Ad-hoc codesign so Virtualization.framework will start a guest.

#[cfg(target_os = "macos")]
pub fn ensure_signed() {
    if std::env::var("SNOWBOX_SIGNED").as_deref() == Ok("1") {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(exe_str) = exe.to_str() else {
        return;
    };
    if has_virtualization_entitlement(exe_str) {
        return;
    }
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
    #[test]
    fn entitlements_plist_declares_virtualization() {
        assert!(
            include_str!("../macos/virtualization.entitlements")
                .contains("com.apple.security.virtualization")
        );
    }
}
