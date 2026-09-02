# Sandbox runtime. systemd stage 1 (the default), image.repart for the
# root disk, kernel loaded by the Host hypervisor — no guest bootloader.
# Do not import the qemu-guest profile: it pulls 9p/virtiofs.
{
  config,
  lib,
  pkgs,
  ...
}:
{
  options.snowbox.control = lib.mkOption {
    type = lib.types.package;
    description = "Guest control plane (snowbox-agent, snowbox-shell).";
  };

  config = {
    system.stateVersion = "26.05";

    boot.loader.external = {
      enable = true;
      installHook = "${pkgs.coreutils}/bin/true";
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
      "virtio_console"
      "vmw_vsock_virtio_transport"
      "vsock"
    ];

    fileSystems."/" = {
      device = "/dev/disk/by-label/nixos";
      fsType = "ext4";
      neededForBoot = true;
    };

    # Bake a small image. Host set_len grows the disk; boot-time
    # systemd-repart grows the GPT/ext4 (SizeMaxBytes here would cap that).
    boot.initrd.systemd.repart.enable = true;
    systemd.repart.partitions."10-root" = {
      Type = "root";
      GrowFileSystem = "yes";
    };

    image.modules.sandbox =
      { modulesPath, ... }:
      {
        imports = [ (modulesPath + "/image/repart.nix") ];
        image.repart = {
          name = "snowbox";
          # Without this, systemd-repart formats a 1 TiB ext4 (67M inodes)
          # before Minimize. The Daemon grows the disk to the Sandbox Limit.
          imageSize = "4G";
          partitions."10-root" = {
            # Outer toplevel, not this extendModules evaluation — init= on
            # the Host cmdline must match the closure on disk.
            storePaths = [ config.system.build.toplevel ];
            repartConfig = {
              Type = "root";
              Format = "ext4";
              Label = "nixos";
              Minimize = "guess";
              SizeMaxBytes = "4G";
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
    nixpkgs.config.allowUnfree = true;

    users.mutableUsers = false;
    users.allowNoPasswordLogin = true;
    users.users.snow = {
      isNormalUser = true;
      extraGroups = [ "wheel" ];
      home = "/home/snow";
      # Login shell for Windows. Written to passwd as
      # /run/current-system/sw/bin/bash. NixOS has no /bin/bash.
      shell = pkgs.bashInteractive;
      # Windows are vsock PTYs, not PAM sessions. Linger keeps /run/user/snow.
      linger = true;
    };
    users.users.root.hashedPassword = "!";
    security.sudo.wheelNeedsPassword = false;
    # Windows are vsock PTYs. Autologin on hvc0 fights the write-only console.

    systemd.tmpfiles.rules = [
      "d /workspace 0755 snow snow -"
    ];

    # Same as a NixOS machine: PATH comes from environment.profiles, not
    # from writing ~/.bashrc. PROFILE symlinks HM home-path (user binaries)
    # when present, else the activation package.
    environment.profiles = lib.mkBefore [ "/nix/var/nix/profiles/snowbox-environment" ];
    environment.sessionVariables.TERM = "xterm-256color";
    environment.sessionVariables.COLORTERM = "truecolor";

    environment.defaultPackages = [ ];
    documentation.enable = false;
    documentation.doc.enable = false;
    documentation.info.enable = false;
    documentation.man.enable = false;
    documentation.nixos.enable = false;
    programs.command-not-found.enable = false;
    programs.nano.enable = false;

    environment.systemPackages = [
      pkgs.nix
      pkgs.util-linux
      pkgs.bashInteractive
    ];

    systemd.services.snowbox-agent = {
      description = "Snowbox control plane";
      wantedBy = [ "multi-user.target" ];
      after = [
        "local-fs.target"
        "systemd-modules-load.service"
      ];
      serviceConfig = {
        ExecStart = "${lib.getExe' config.snowbox.control "snowbox-agent"}";
        Restart = "always";
        Environment = "PATH=${
          lib.makeBinPath [
            pkgs.nix
            pkgs.util-linux
            pkgs.bash
            pkgs.coreutils
          ]
        }";
      };
    };

    # Window shells. The Daemon bridges a Host WebSocket to this vsock;
    # the browser never talks to the guest. Each connect is a login shell
    # (login_tty, then execve of snow's passwd shell with argv0 `-bash`).
    systemd.services.snowbox-shell = {
      description = "Snowbox Window shells";
      wantedBy = [ "multi-user.target" ];
      after = [
        "local-fs.target"
        "systemd-modules-load.service"
      ];
      serviceConfig = {
        ExecStart = "${lib.getExe' config.snowbox.control "snowbox-shell"}";
        Restart = "always";
        Environment = "PATH=${
          lib.makeBinPath [
            pkgs.util-linux
            pkgs.coreutils
          ]
        }";
      };
    };
  };
}
