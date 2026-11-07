# The hermes-desktop launcher picks the Wayland or X11 ozone backend from the
# session environment. Built against a stub app that only echoes its argv.
{ pkgs, runCommand }:
let
  stub = pkgs.writeShellScriptBin "hermes-desktop" ''echo "$@"'';
  launcher = (pkgs.callPackage ../desktop.nix { hermesDesktop = stub; }).hermes-desktop;
in
runCommand "check-desktop-launcher" { } ''
  run() { env -i "$@" ${launcher}/bin/hermes-desktop --probe; }
  expect() { want=$1; shift; got=$(run "$@"); case "$got" in
    *"--ozone-platform=$want --probe"*) echo "[+] $* -> $want" ;;
    *) echo "[-] $* -> '$got', expected $want"; exit 1 ;; esac; }
  expect wayland WAYLAND_DISPLAY=wayland-0
  expect wayland XDG_SESSION_TYPE=wayland
  expect x11 XDG_SESSION_TYPE=x11 DISPLAY=:0
  expect x11 DISPLAY=:0
  expect x11
  touch $out
''
