{
  description = "Snowbox sandbox runtime (NixOS 26.05, aarch64-linux)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { nixpkgs, ... }:
    let
      system = "aarch64-linux";
      lib = nixpkgs.lib;
      nixos = nixpkgs.lib.nixosSystem {
        inherit system;
        modules = [ ./module.nix ];
      };
      pkgs = nixos.pkgs;
      kernelFile = pkgs.stdenv.hostPlatform.linux-kernel.target;
      cmdline = lib.concatStringsSep " " nixos.config.boot.kernelParams;
      image = nixos.config.system.build.images.sandbox;
    in
    {
      nixosConfigurations.sandbox = nixos;
      packages.${system} = rec {
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
        default = runtime;
      };
    };
}
