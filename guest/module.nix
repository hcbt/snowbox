# Sandbox runtime. NixOS 26.05: systemd stage 1 (the default), image.repart
# for the root disk, kernel loaded by the Host hypervisor — no guest
# bootloader. Do not import the qemu-guest profile: it pulls 9p/virtiofs.
{
  config,
  lib,
  pkgs,
  ...
}:
{
  nixpkgs.hostPlatform = "aarch64-linux";
  system.stateVersion = "26.05";

  boot.loader.external = {
    enable = true;
    installHook = pkgs.writeScript "snowbox-no-bootloader" ''
      #!${pkgs.runtimeShell}
      exit 0
    '';
  };

  boot.kernelParams = [
    "console=hvc0"
    "systemd.journald.forward_to_console=1"
  ];

  boot.initrd.availableKernelModules = [
    "virtio_net"
    "virtio_pci"
    "virtio_mmio"
    "virtio_blk"
    "virtio_console"
    "vmw_vsock_virtio_transport"
    "vsock"
    "ext4"
  ];
  boot.kernelModules = [
    "vmw_vsock_virtio_transport"
    "vsock"
  ];

  fileSystems."/" = {
    device = "/dev/disk/by-label/nixos";
    fsType = "ext4";
    neededForBoot = true;
  };

  image.modules.sandbox =
    { config, modulesPath, ... }:
    {
      imports = [ (modulesPath + "/image/repart.nix") ];
      image.repart = {
        name = "snowbox";
        partitions."10-root" = {
          storePaths = [ config.system.build.toplevel ];
          repartConfig = {
            Type = "root";
            Format = "ext4";
            Label = "nixos";
            Minimize = "guess";
          };
        };
      };
    };

  networking.hostName = "sandbox";
  networking.useNetworkd = true;
  networking.firewall.enable = false;
  systemd.network.enable = true;
  systemd.network.networks."20-virtio" = {
    matchConfig.Driver = "virtio_net";
    networkConfig.DHCP = "yes";
  };

  nix.enable = true;
  nix.channel.enable = false;
  nix.settings.experimental-features = [
    "nix-command"
    "flakes"
  ];

  users.mutableUsers = false;
  users.allowNoPasswordLogin = true;
  users.users.snow = {
    isNormalUser = true;
    extraGroups = [ "wheel" ];
    home = "/home/snow";
  };
  users.users.root.hashedPassword = "!";
  security.sudo.wheelNeedsPassword = false;
  services.getty.autologinUser = "snow";

  systemd.tmpfiles.rules = [
    "d /workspace 0755 snow snow -"
  ];

  environment.defaultPackages = [ ];
  documentation.enable = false;
  documentation.doc.enable = false;
  documentation.info.enable = false;
  documentation.man.enable = false;
  documentation.nixos.enable = false;
  programs.command-not-found.enable = false;
  programs.nano.enable = false;

  environment.systemPackages = [
    pkgs.socat
    pkgs.gnutar
    pkgs.gzip
  ];

  systemd.services.snowbox-agent = {
    description = "Snowbox control plane";
    wantedBy = [ "multi-user.target" ];
    after = [ "local-fs.target" ];
    serviceConfig = {
      ExecStart = "${pkgs.socat}/bin/socat VSOCK-LISTEN:52,reuseaddr,fork EXEC:${lib.getExe config.system.build.snowbox-agent}";
      Restart = "always";
    };
  };

  # Window shells. The Daemon bridges a Host WebSocket to this vsock;
  # the browser never talks to the guest. Each connect is a login shell
  # for snow (closing the Window ends that shell).
  systemd.services.snowbox-shell = {
    description = "Snowbox Window shells";
    wantedBy = [ "multi-user.target" ];
    after = [ "local-fs.target" ];
    serviceConfig = {
      ExecStart = "${pkgs.socat}/bin/socat VSOCK-LISTEN:53,reuseaddr,fork EXEC:${lib.getExe config.system.build.snowbox-shell},pty,stderr,setsid,sigint,sane,ctty";
      Restart = "always";
    };
  };

  system.build.snowbox-shell = pkgs.writeShellApplication {
    name = "snowbox-shell";
    runtimeInputs = [
      pkgs.util-linux
      pkgs.bash
    ];
    text = ''
      exec runuser -u snow -- ${pkgs.bash}/bin/bash -l
    '';
  };

  system.build.snowbox-agent = pkgs.writeShellApplication {
    name = "snowbox-agent";
    runtimeInputs = [
      pkgs.coreutils
      pkgs.gnutar
      pkgs.gzip
      pkgs.nix
    ];
    text = ''
      set -euo pipefail
      read -r cmd arg || true
      case "$cmd" in
        PING)
          printf 'PONG\n'
          ;;
        TAR_IN)
          mkdir -p "$arg"
          tar -C "$arg" --no-same-owner -xf -
          chown -R snow:snow "$arg" 2>/dev/null || true
          printf 'OK\n'
          ;;
        TAR_OUT)
          tar -C "$arg" -cf - .
          ;;
        NAR_IN)
          nix-store --import >/dev/null
          printf 'OK\n'
          ;;
        PROFILE)
          mkdir -p /nix/var/nix/profiles /etc/profile.d /home/snow
          ln -sfn "$arg" /nix/var/nix/profiles/snowbox-environment
          ln -sfn "$arg" /home/snow/.nix-profile
          printf "export PATH=/nix/var/nix/profiles/snowbox-environment/bin:\$PATH\n" >/etc/profile.d/snowbox-environment.sh
          chown -h snow:users /home/snow/.nix-profile 2>/dev/null || true
          printf 'OK\n'
          ;;
        CONNECT)
          exec ${pkgs.socat}/bin/socat STDIO TCP:127.0.0.1:"$arg"
          ;;
        *)
          printf 'ERR unknown\n' >&2
          exit 1
          ;;
      esac
    '';
  };
}
