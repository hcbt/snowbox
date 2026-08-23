# snowbox

A local Nix program that runs a coding agent inside an isolated Linux sandbox on your machine.

v1 is macOS-first (Virtualization.framework). Enter the env with [devenv](https://devenv.sh/):

```
devenv shell -- nix run .#spike-a -- prove
```

Domain language is in [CONTEXT.md](CONTEXT.md). Design decisions are in [docs/adr/](docs/adr/).
