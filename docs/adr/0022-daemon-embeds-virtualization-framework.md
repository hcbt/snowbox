# The Daemon embeds Virtualization.framework

vfkit is a Go CLI that wraps Virtualization.framework. Spawning it from the Rust Daemon makes the VMM a second process and a command-line ABI. vfkit’s Go package is not an embeddable VMM; it generates that CLI.

The macOS VMM is in-process: [objc2-virtualization](https://crates.io/crates/objc2-virtualization) talks to Virtualization.framework from the Daemon. No vfkit. The snowbox binary needs the `com.apple.security.virtualization` entitlement and must be signed before a guest will start. `VZVirtualMachine` runs on a dedicated thread with a run loop; that stays inside the Daemon.

libkrun and QEMU stay out as the macOS default ([0010](0010-macos-uses-virtualization-framework.md)). Linux KVM is a later Host.
