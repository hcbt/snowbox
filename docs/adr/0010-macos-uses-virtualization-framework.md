# macOS isolation is Virtualization.framework

The first Host is macOS. Firecracker is not that Host. QEMU-as-default makes the product feel like a lab VM.

The first macOS backend is Virtualization.framework. The Daemon calls it in-process ([0022](0022-daemon-embeds-virtualization-framework.md)). There is Firecracker-shaped work on top of VF (libkrun and similar); that is research, not a second v1 product. Linux KVM remains the other Host, later in v1. The UX does not wait on a hypervisor abstraction layer.
