# Copy-in, copy-out, and Publish are not Snowbox verbs

Copy-in/out and Publish shipped without a domain decision. Workspace files enter from inside the Sandbox. Destroy always deletes the Workspace. Guest ports stay closed to the Host; SSH remains a Unix side effect, not a feature.

This supersedes [0007](0007-explicit-port-publish.md).
