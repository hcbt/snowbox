# The Cache is a Host store; Sandboxes copy from it

Re-downloading nixpkgs for every Sandbox is waste. A shared `/nix/store` mount would punch neighbor isolation and show Sandbox A what B already fetched. Using the Host user’s Nix store would leak that store into guests.

Snowbox keeps its own Cache on the Host, separate from the user’s Nix store. The Daemon is the only writer: when it realizes an Environment, paths land in the Cache. Each Sandbox has its own store and copies from the Cache instead of the network. Guests may use the Cache as a read substituter (including in-guest `nix build`); they cannot publish into it. Guest cache misses may still hit the internet; that does not warm the Cache.

The Cache is a real Nix `file://` binary cache, not a write-only NAR dump: `nix-cache-info` (`StoreDir`, `WantMassQuery`, `Priority`), one `.narinfo` per realized path, and `nar/` named as Nix expects (`<narhash>.nar`). Guests still receive a vsock `NAR_IN` apply; `file://` is Host-side correctness and a later in-guest substituter.

The copy channel is a Daemon control plane (vsock is fine), not a virtiofs of the Cache.
