{ pkgs, ... }:
let
  raw = builtins.fromJSON (builtins.readFile ./config.json);
  programs = raw.programs or { };
  take = name: programs.${name} or { enable = false; };
in
{
  home.username = "snow";
  home.homeDirectory = "/home/snow";
  home.stateVersion = "26.05";
  programs.bash.enable = true;
  home.packages = [ pkgs.devenv ];
  programs.claude-code = take "claude-code";
  programs.codex = take "codex";
  programs.pi-coding-agent = take "pi-coding-agent";
}
