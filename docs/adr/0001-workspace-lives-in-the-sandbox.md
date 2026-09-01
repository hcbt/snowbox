# Workspace lives on the Sandbox disk

A coding Agent needs project files, and almost every agent-sandbox mounts the Host repo into the VM. That makes `rm -rf` on the project a Host deletion and punches a hole in the isolation claim.

Snowbox keeps the Workspace on the Sandbox disk instead. The Host never has a live view. Files enter from inside the Sandbox. Reset restores the declared Environment and leaves the Workspace in place.

The cost is a blind Host editor unless the user SSH/VS Code Remotes in themselves (not a product feature). The gain is an isolation claim we can actually defend: the Host is untouched except whatever the user forwards.
