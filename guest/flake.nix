{
  description = "Snowbox sandbox runtime (NixOS 26.05)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { nixpkgs, ... }:
    let
      lib = nixpkgs.lib;
      mk =
        system:
        let
          nixos = nixpkgs.lib.nixosSystem {
            inherit system;
            modules = [ ./module.nix ];
          };
          pkgs = nixos.pkgs;
          kernelFile = pkgs.stdenv.hostPlatform.linux-kernel.target;
          cmdline = lib.concatStringsSep " " (
            [ "init=${nixos.config.system.build.toplevel}/init" ] ++ nixos.config.boot.kernelParams
          );
          image = nixos.config.system.build.images.sandbox;
        in
        rec {
          inherit nixos;
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
          ;
        default = runtime;
      };
    };
}
