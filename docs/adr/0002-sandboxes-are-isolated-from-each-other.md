# Sandboxes are isolated from each other

v1 is one person on one Host, but more than one Sandbox may run at a time (two projects, two boxes). The cheap design is a shared bridge and a shared folder.

Snowbox does not do that. Concurrent Sandboxes share Host CPU, RAM, and disk capacity, and nothing else: no Sandbox-to-Sandbox filesystem, no Sandbox-to-Sandbox network.

They do not share a `/nix/store` mount. The allowed exception is the **Cache**: a Host-side store the Daemon writes and Sandboxes only copy from. That is not a neighbor filesystem; it is the Host handing out already-fetched Packages.

That is the same neighbor boundary a later platform will need. Linking Sandboxes is a different product.
