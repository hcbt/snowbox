//! Virtualization.framework, in-process, macOS only.

#[cfg(target_os = "macos")]
pub fn is_supported() -> bool {
    use objc2_virtualization::VZVirtualMachine;
    unsafe { VZVirtualMachine::isSupported() }
}

#[cfg(not(target_os = "macos"))]
pub fn is_supported() -> bool {
    false
}

#[cfg(test)]
mod tests {
    #[test]
    fn reports_support_without_creating_a_vm() {
        let _ = super::is_supported();
    }
}
