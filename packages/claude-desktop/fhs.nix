{
  lib,
  runCommand,
  buildFHSEnv,
  bubblewrap,
  claude-desktop,
  nodejs,
  docker,
  docker-compose,
  openssl,
  glibc,
  uv,
  OVMF,
  qemu_kvm,
  virtiofsd,
}:

let
  # Cowork's local-VM helper (coworkd, bundled inside claude-desktop as
  # `resources/cowork-linux-helper`) gates support on three probes that
  # all assume a Debian/Ubuntu FHS layout, none of which exist on plain
  # NixOS:
  #
  #   1. `qemu-system-x86_64` on $PATH              -> qemu_kvm below
  #   2. `/usr/bin/virtiofsd` (or /usr/libexec/...)  -> virtiofsd below
  #   3. `/usr/share/OVMF/OVMF_CODE*.fd` + a writable
  #      VARS sibling next to it (derived by renaming
  #      OVMF_CODE -> OVMF_VARS)                     -> ovmfCompat shim
  #
  # /dev/kvm and vhost-vsock are reachable inside the FHS env because
  # buildFHSEnv bind-mounts the whole host /dev; the host still needs to
  # grant kvm-group access (/dev/kvm readable/writable by the user) and
  # have the vhost_vsock kernel module loaded.
  #
  # nixpkgs' OVMF lands firmware at ${OVMF.fd}/FV/*.fd -- nothing under
  # share/ -- so a bare OVMF in targetPkgs never satisfies probe #3;
  # symlink the FV/ files into share/OVMF, which buildFHSEnv maps to
  # /usr/share/OVMF inside the sandbox. x86_64 only needs the CODE/VARS
  # pair aliased under both the 4M and non-4M names Cowork's probe list
  # checks.
  ovmfCompat = runCommand "claude-desktop-ovmf-compat" { } ''
    mkdir -p "$out/share/OVMF"
    ln -s ${OVMF.fd}/FV/OVMF_CODE.fd "$out/share/OVMF/OVMF_CODE.fd"
    ln -s ${OVMF.fd}/FV/OVMF_CODE.fd "$out/share/OVMF/OVMF_CODE_4M.fd"
    ln -s ${OVMF.fd}/FV/OVMF_VARS.fd "$out/share/OVMF/OVMF_VARS.fd"
    ln -s ${OVMF.fd}/FV/OVMF_VARS.fd "$out/share/OVMF/OVMF_VARS_4M.fd"
  '';
in
buildFHSEnv {
  name = "claude-desktop";

  # docker/docker-compose/nodejs/uv/openssl/glibc are here for MCP
  # servers Cowork/Claude Code may spawn via npx/uvx/docker inside this
  # FHS env (same rationale as cursor-ide's FHS-adjacent PATH wiring),
  # not strictly required for the VM itself.
  targetPkgs = _pkgs: [
    bubblewrap
    claude-desktop
    docker
    docker-compose
    glibc
    nodejs
    openssl
    ovmfCompat
    qemu_kvm
    uv
    virtiofsd
  ];

  runScript = "${claude-desktop}/bin/claude-desktop";

  extraInstallCommands = ''
    mkdir -p $out/share/applications
    cp ${claude-desktop}/share/applications/* $out/share/applications/
    mkdir -p $out/share/icons
    cp -r ${claude-desktop}/share/icons/* $out/share/icons/
  '';

  meta = claude-desktop.meta // {
    description = "Claude Desktop for Linux (FHS environment) -- enables Cowork's local QEMU/KVM sandbox and MCP servers via npx/uvx/docker";
  };
}
