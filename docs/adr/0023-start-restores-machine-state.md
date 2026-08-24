# Start restores machine state

Cold-booting NixOS on every Start is several seconds (kernel, initrd, userspace). Sandboxes that feel instant restore a guest that is already past that.

Stop writes Virtualization.framework machine state next to the Sandbox disk and tears the VM down. Start restores that file onto **the same disk** and resumes. After a successful restore the save file (`.vzvmsave`) is deleted: the disk has moved on, and a crash mid-Environment apply must not restore old RAM onto a newer disk.

The platform `machineIdentifier` is persisted with the Sandbox (`machine.ident`). Restore fails with `invalid argument` if it does not match the save. Two running guests must not share an identifier.

Ready is warmed in the background from `.warm` only (disk + ident), keyed by the guest runtime, not by Environment. Stop writes machine state for that Sandbox; it does not bake `.ready`. A New Sandbox consumes the snapshot and restores — about a second, then Environment is applied if the stamp differs. The Daemon boots a replacement in the background. Two running guests never share an identifier.

If Apple refuses the device set (`validateSaveRestoreSupport`), or restore fails, or the agent does not answer, Start boots. `validateSaveRestoreSupport` failing does not block boot; Stop then skips writing machine state.

Boot and restore use one configuration: same device order, persisted MAC, virtio-blk Cached+Full, no entropy device, one virtio-socket, balloon, and a serial console. Both attach `VZFileSerialPortAttachment` (file URL, append) to `console.log`. A new `NSFileHandle` each Start is not restore-compatible; the URL attachment is.

The VM object is retained until `stopWithCompletionHandler` runs. A stop timeout does not drop it.

Isolation does not change: each Sandbox still has its own disk and its own NAT. `vmnet_network` is CF-retained, keyed by Sandbox UUID (not snapshot MAC), and kept until Destroy or process exit so same-process restore can reuse it. `vmnet_network_ref` is same-process only — creating a new `SHARED_MODE` (1001) object after a Daemon restart is not restore-compatible. Status 1002 is `VMNET_MEM_FAILURE`, not a wrong mode.

GUI nouns stay Start and Stop.
