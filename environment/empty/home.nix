{ pkgs, ... }:
let
  raw = builtins.fromJSON (builtins.readFile ./config.json);
  programs = raw.programs or { };
  take = name: programs.${name} or { enable = false; };
  withPkgs =
    cfg:
    let
      names = cfg.extraPackages or [ ];
    in
    (builtins.removeAttrs cfg [ "extraPackages" ])
    // {
      extraPackages = map (n: pkgs.${n}) names;
    };
in
{
  home.username = "snow";
  home.homeDirectory = "/home/snow";
  home.stateVersion = "26.05";
  programs.bash.enable = true;
  programs.bash.initExtra = ''
    if [ -f "$HOME/.snowbox-env" ]; then
      . "$HOME/.snowbox-env"
    fi
  '';
  home.packages = [ pkgs.devenv ];
  programs.claude-code = take "claude-code";
  programs.codex = take "codex";
  programs.pi-coding-agent = withPkgs (take "pi-coding-agent");
}
