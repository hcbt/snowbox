{
  description = "Snowbox Environment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { nixpkgs, ... }:
    let
      system = "aarch64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      names = builtins.fromJSON (builtins.readFile ./packages.json);
    in
    {
      packages.${system}.default = pkgs.buildEnv {
        name = "environment";
        paths = map (n: pkgs.${n}) names;
      };
    };
}
