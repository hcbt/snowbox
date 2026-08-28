# The Environment lives on the Host

The Agent has passwordless sudo, so anything *inside* the Sandbox that counts as the Environment declaration can be rewritten into a declared Package. Reset would then keep `nasty`. Putting the Environment flake in `/workspace` also confuses it with the project’s own flake.

The Environment is a Host document owned by Snowbox. The GUI and Template overlay write it; the Sandbox only receives realized Packages. A flake in `/workspace` is the project’s. The Agent cannot persist a Package add.
