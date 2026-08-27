# Host coding-agent skills come through APM

Skills and prompts for *developing this repo* (not Agents inside a Sandbox) are declared in `apm.yml` and installed with `pkgs.apm-cli`. A clone gets the same files; a local Grok plugin set is not the source of truth for other people. Product rules stay in the hand-authored `AGENTS.md`; APM only writes the managed section between `<!-- apm:start -->` and `<!-- apm:end -->`.

The pin of `apm-cli` in devenv-nixpkgs is 0.21.0, which has no `grok-build` or `agent-skills` target. Skills deploy through `copilot` into `.agents/skills/`.

Canvas lint is oxlint and oxfmt from devenv (`.oxlintrc.json` and `.oxfmtrc.json` at the repo root). The only bun lint dependency is `@oxlint/plugins`, which the vendored anti-slop plugin imports and nixpkgs does not package.
