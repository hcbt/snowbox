# Host-side shell. Everyday utilities are pinned so `nix develop` /
# `devenv shell` do not fall through to Homebrew.
{ pkgs, ... }:
{
  languages.rust.enable = true;

  # Bun is the JS toolchain for the Canvas (ADR 0021). Do not fall through
  # to a host node/npm. bun.install waits until there is a package.json.
  languages.javascript.enable = true;
  languages.javascript.bun.enable = true;

  packages = [
    pkgs.git
    pkgs.gh
    pkgs.coreutils
    pkgs.gnugrep
    pkgs.gnused
    pkgs.findutils
    pkgs.curl
    pkgs.pkg-config
    pkgs.nix.dev
    pkgs.llvmPackages.libclang
  ];

  env.LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";

  processes.snowbox.exec = "cargo run -p snowbox";

  # Guest runtime is aarch64-linux (NixOS 26.05). On Darwin this goes
  # through linux-builder. The Daemon looks at guest/result or SNOWBOX_RUNTIME.
  scripts.guest.exec = "nix build path:./guest#packages.aarch64-linux.runtime --out-link guest/result";

  git-hooks.package = pkgs.prek;
  git-hooks.hooks = {
    nixfmt-rfc-style.enable = true;
    nixfmt-rfc-style.package = pkgs.nixfmt;
    check-merge-conflicts.enable = true;
    check-yaml.enable = true;
    check-added-large-files.enable = true;
    end-of-file-fixer.enable = true;
    trim-trailing-whitespace.enable = true;
  };

  git-hooks.excludes = [
    "^LICENSE$"
    "\\.lock$"
  ];
}
