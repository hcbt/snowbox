# Home is an allowlist, not $HOME

Reset must keep Agent logins and drop undeclared tools. Those both normally live under the Unix home directory (`npm i -g` → `~/.local`, `gh` auth → `~/.config/gh`). Preserving all of `$HOME` makes Reset a no-op for malware. Wiping `$HOME` makes Reset drop every login.

Home is therefore an allowlist of paths. v1: known Agent CLI auth dirs as each first-class Agent is added, `.gitconfig`, and Secrets Snowbox placed. Not all of `~/.config`. Install prefixes are not Home. A new Agent whose auth path is not on the list loses login on Reset; that is cheaper than a denylist.
