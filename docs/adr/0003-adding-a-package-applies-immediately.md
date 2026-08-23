# Adding a Package applies immediately

The Environment is a declaration. It is easy to treat “add jq” as editing a list and applying it on the next Reset or reboot. Then the Agent’s next command fails, and Reset becomes the install button.

Snowbox updates the Environment *and* the running Sandbox when a Package is added. Reset still exists, but for undeclared mutations, not for installing from the catalog.

The cost is that live-switch has to actually work. If it does not, the catalog is a lie.

Spike B on this Mac: adding `hello` to a running guest is `nix copy` from the Cache plus `nix-env`. No reboot. `hello` ran in the same VM. Reset still dropped an undeclared `~/.local/bin/nasty` and kept `/workspace` and `.gitconfig`.
