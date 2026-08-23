# Start restores machine state

Cold-booting NixOS on every Start is several seconds (kernel, initrd, userspace). Sandboxes that feel instant restore a guest that is already past that.

Stop writes Virtualization.framework machine state next to the Sandbox disk and tears the VM down. Start restores that file onto **the same disk** and resumes.

The platform `machineIdentifier` is persisted with the Sandbox (`machine.ident`). Restore fails with `invalid argument` if it does not match the save. Two running guests must not share an identifier.

Stop bakes that save (disk + ident) as a ready snapshot keyed by the guest runtime, not by Environment. A New Sandbox consumes the snapshot and restores — about a second, then Environment is applied if the stamp differs. The snapshot ident is then live, so a second New Sandbox while the first is running boots. The next Stop puts a free ident back in the cache.

If Apple refuses the device set (`validateSaveRestoreSupport`), or restore fails, or the agent does not answer, Start boots.

Isolation does not change: each Sandbox still has its own disk and its own NAT. vmnet objects are kept for the process lifetime so restore can reuse the same network. Serial consoles are omitted; a new file-handle each Start also breaks restore.

GUI nouns stay Start and Stop.
