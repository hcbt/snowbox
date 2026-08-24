# Host-side shell. Everyday utilities are pinned so `nix develop` /
# `devenv shell` do not fall through to Homebrew.
{ pkgs, ... }:
let
  guestSystem =
    if pkgs.stdenv.hostPlatform.isDarwin then "aarch64-linux" else pkgs.stdenv.hostPlatform.system;
  # Canvas + guest (if missing) + Daemon. `devenv up` / `devenv shell -- snowbox`.
  stack = ''
    set -euo pipefail
    # Rebuild when the baked runtime is missing or guest sources are newer.
    # `devenv shell -- guest` always rebuilds.
    if [ ! -f guest/result/root.raw ] \
      || [ guest/module.nix -nt guest/result/root.raw ] \
      || [ guest/flake.nix -nt guest/result/root.raw ] \
      || find guest/control/src guest/control/Cargo.toml guest/control/Cargo.lock \
        -newer guest/result/root.raw 2>/dev/null | grep -q .; then
      nix build path:./guest#packages.${guestSystem}.runtime --out-link guest/result
    fi
    ( cd canvas && bun install && bun run build )
    cargo build -p snowbox-eval
    export SNOWBOX_EVAL="$PWD/target/debug/snowbox-eval"
    export SNOWBOX_RUNTIME="$PWD/guest/result"
    exec cargo run -p snowbox
  '';
in
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
  ]
  ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.qemu ];

  env.LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";

  processes.snowbox.exec = stack;
  scripts.snowbox.exec = stack;

  scripts.canvas.exec = "cd canvas && bun install && bun run build";

  # Guest runtime tracks nixpkgs-unstable. Darwin builds aarch64-linux through
  # linux-builder; Linux builds the host architecture. The Daemon looks
  # at guest/result or SNOWBOX_RUNTIME.
  scripts.guest.exec = "nix build path:./guest#packages.${guestSystem}.runtime --out-link guest/result";

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
