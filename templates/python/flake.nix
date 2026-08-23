{
  description = "Snowbox Environment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      system = "aarch64-linux";
      pkgs = import nixpkgs {
        inherit system;
        config.allowUnfree = true;
      };
      names = builtins.fromJSON (builtins.readFile ./packages.json);
    in
    {
      packages.${system}.default = pkgs.buildEnv {
        name = "environment";
        paths = map (n: pkgs.${n}) names;
      };
    };
}
