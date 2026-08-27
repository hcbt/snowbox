{
  pkgs,
  lib,
  config,
  inputs,
  ...
}:
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
  # https://devenv.sh/basics/
  env.GREET = "devenv";
  env.LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
  # uv uses the nix-provided Python; never download an interpreter.
  env.UV_PYTHON_DOWNLOADS = "never";
  env.UV_PYTHON_PREFERENCE = "only-system";

  # https://devenv.sh/packages/
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
    pkgs.bun
    pkgs.apm-cli
    pkgs.oxlint
    pkgs.oxfmt
  ]
  ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.qemu ];

  # https://devenv.sh/languages/
  languages.rust.enable = true;

  # Canvas is TypeScript, built with Bun (ADR 0021). devenv nests bun under
  # languages.javascript, which we do not enable; bun is in packages.
  languages.typescript.enable = true;

  # Python 3.12; uv manages the packages.
  languages.python = {
    enable = true;
    package = pkgs.python312;
    uv.enable = true;
  };

  # https://devenv.sh/processes/
  processes.snowbox.exec = stack;

  # https://devenv.sh/services/
  # services.postgres.enable = true;

  # https://devenv.sh/scripts/
  scripts.hello.exec = ''
    echo hello from $GREET
  '';
  scripts.snowbox.exec = stack;
  scripts.canvas.exec = "cd canvas && bun install && bun run build";
  # Guest runtime tracks nixpkgs-unstable. Darwin builds aarch64-linux through
  # linux-builder; Linux builds the host architecture. The Daemon looks
  # at guest/result or SNOWBOX_RUNTIME.
  scripts.guest.exec = "nix build path:./guest#packages.${guestSystem}.runtime --out-link guest/result";

  # https://devenv.sh/basics/
  enterShell = ''
    hello         # Run scripts directly
    git --version # Use packages
  '';

  # https://devenv.sh/tasks/
  # tasks = {
  #   "myproj:setup".exec = "mytool build";
  #   "devenv:enterShell".after = [ "myproj:setup" ];
  # };

  # https://devenv.sh/tests/
  enterTest = ''
    echo "Running tests"
    git --version | grep --color=auto "${pkgs.git.version}"
  '';

  # https://devenv.sh/git-hooks/
  git-hooks.package = pkgs.prek;
  git-hooks.hooks = {
    nixfmt-rfc-style.enable = true;
    nixfmt-rfc-style.package = pkgs.nixfmt;
    check-merge-conflicts.enable = true;
    check-yaml.enable = true;
    check-added-large-files.enable = true;
    end-of-file-fixer.enable = true;
    trim-trailing-whitespace.enable = true;
    oxlint.enable = true;
    oxlint.files = "^canvas/src/.*\\.(js|jsx|ts|tsx)$";
    oxlint.settings.configPath = "./.oxlintrc.json";
    oxfmt.enable = true;
    oxfmt.files = "^canvas/src/.*\\.(js|jsx|ts|tsx)$";
    oxfmt.settings.mode = "check";
  };
  git-hooks.excludes = [
    "^LICENSE$"
    "\\.lock$"
    "^canvas/tools/"
    "^apm_modules/"
  ];

  # See full reference at https://devenv.sh/reference/options/
}
