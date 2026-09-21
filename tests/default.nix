# nixos vm tests. every one of these boots a real disk image through limine
# rather than being handed a kernel by qemu, so reaching userspace is itself
# the assertion that the install worked.
{
  pkgs,
  module,
}:
let
  tests = [
    "removable"
    "registered"
  ];

  runTest = name: pkgs.testers.runNixOSTest (import ./${name}.nix { inherit module; });
in
pkgs.lib.genAttrs tests runTest
