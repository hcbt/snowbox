# Templates are home-manager Agent configurations

A Template that is a list of nixpkgs Packages is unused: the Agent installs what a project needs with devenv inside the Workspace. The Canvas catalog was that list, as a lobby. Both go.

A Template is still a Nix flake ([0005](0005-templates-are-nix-flakes.md)). What it means is devenv plus a home-manager configuration of first-class Agents (`programs.claude-code`, `programs.pi-coding-agent`, `programs.codex`, …). The Environment form is a Canvas form over those home-manager options. Which Agents appear, and which of their options the form draws, is `environment/empty/form.json` in the Snowbox repo — not copied into each Sandbox, not queried from home-manager when the Daemon starts. Adding an Agent is editing that file when the module exists. Each listed option is its own control; JSON-typed options stay one JSON box each. The form’s Host document is JSON (`config.json`); `home.nix` reads it. HTTP is JSON because the Canvas is not a Nix editor. Save still writes the flake (JSON + the Nix that reads it); New Sandbox realizes it.

devenv is always in the Environment. It is not a Template choice and not baked into the guest OS image (OS pin and tools pin stay separate). A devenv in `/workspace` is the project’s, not the Environment ([0009](0009-environment-lives-on-the-host.md)).

This supersedes [0003](0003-adding-a-package-applies-immediately.md) (no catalog, no live Package add) and the “no vendor list” sentence of [0013](0013-agents-are-uncapped-per-sandbox.md). Vendors appear as Template options; the product noun stays Agent. Snowbox may still start zero or one command.

Secrets do not belong in the flake: home-manager `apiKey` options would land in the Cache. Placing a Secret is later.
