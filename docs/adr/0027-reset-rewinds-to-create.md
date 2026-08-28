# Reset rewinds to Create

Reset is not “apply the current Environment and keep a Home allowlist.” That keep-list was the complexity: special paths (`.gitconfig`, Agent logins) survived while undeclared tools did not.

Reset puts this Sandbox back to Create: the Environment as it was at that moment (including Customize at Create), an empty Linux home, the same Workspace. Logins and extra installs are gone. It does not read the Template as it exists now. Destroy is what deletes the Workspace.

This supersedes [0004](0004-home-is-an-allowlist.md). Home is the Linux user home inside the Sandbox; Stop keeps it on disk; Reset wipes it. There is no keep-list.
