{
  description = "Snowbox sandbox runtime";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      lib = nixpkgs.lib;
      mkControl =
        pkgs:
        pkgs.rustPlatform.buildRustPackage {
          pname = "snowbox-guest";
          version = "0.0.0";
          src = lib.cleanSourceWith {
            src = ./control;
            filter =
              path: type:
              let
                base = baseNameOf path;
              in
              base != "target" && !(lib.hasInfix "/target/" path);
          };
          cargoLock.lockFile = ./control/Cargo.lock;
          doCheck = true;
        };
      mk =
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          control = mkControl pkgs;
          nixos = nixpkgs.lib.nixosSystem {
            inherit system;
            modules = [
              ./module.nix
              { snowbox.control = control; }
            ];
          };
          kernelFile = nixos.config.system.boot.loader.kernelFile;
          cmdline = lib.concatStringsSep " " (
            [ "init=${nixos.config.system.build.toplevel}/init" ] ++ nixos.config.boot.kernelParams
          );
          image = nixos.config.system.build.images.sandbox;
        in
        rec {
          inherit nixos control;
          kernel = nixos.config.system.build.kernel;
          initrd = nixos.config.system.build.initialRamdisk;
          toplevel = nixos.config.system.build.toplevel;
          rootfs = image;
          runtime = pkgs.runCommand "snowbox-runtime" { } ''
            mkdir -p $out
            cp -L ${kernel}/${kernelFile} $out/kernel
            cp -L ${initrd}/initrd $out/initrd
            cp -L ${image}/*.raw $out/root.raw
            printf '%s\n' ${lib.escapeShellArg cmdline} > $out/cmdline
          '';
        };
      aarch64 = mk "aarch64-linux";
      x86_64 = mk "x86_64-linux";
    in
    {
      nixosConfigurations.sandbox = aarch64.nixos;
      nixosConfigurations.sandbox-x86_64-linux = x86_64.nixos;
      packages.aarch64-linux = rec {
        inherit (aarch64)
          kernel
          initrd
          toplevel
          rootfs
          runtime
          control
          ;
        default = runtime;
      };
      packages.x86_64-linux = rec {
        inherit (x86_64)
          kernel
          initrd
          toplevel
          rootfs
          runtime
          control
          ;
        default = runtime;
      };
    };
}
