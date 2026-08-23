{
  description = "Snowbox: coding Agents in isolated Nix-built Linux Sandboxes";

  # Packaging for `nix run` goes here when we ship. Day-to-day is devenv
  # (`devenv.nix`): enter the shell, run the Daemon from there.
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }: { };
}
