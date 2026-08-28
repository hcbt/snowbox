{
  description = "Snowbox Environment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  inputs.home-manager.url = "github:nix-community/home-manager";
  inputs.home-manager.inputs.nixpkgs.follows = "nixpkgs";

  outputs =
    { nixpkgs, home-manager, ... }:
    let
      system = "aarch64-linux";
      pkgs = import nixpkgs {
        inherit system;
        config.allowUnfree = true;
      };
      hm = home-manager.lib.homeManagerConfiguration {
        inherit pkgs;
        modules = [ ./home.nix ];
      };
    in
    {
      packages.${system}.default = hm.activationPackage;
      agentOptions = builtins.toJSON (
        import ./dump-options.nix {
          inherit (pkgs) lib;
          options = hm.options.programs;
        }
      );
    };
}
