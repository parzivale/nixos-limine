# limine installed under its own directory and registered in NVRAM. The
# removable fallback path is deliberately absent, so the firmware can only
# find limine through the boot entry the installer wrote.
{ module }:
{
  name = "limine-registered";

  nodes.machine =
    { pkgs, ... }:
    {
      imports = [ module ];

      virtualisation.useBootLoader = true;
      virtualisation.useEFIBoot = true;

      boot.loader.efi.canTouchEfiVariables = true;

      boot.loader.grub.enable = false;

      boot.loader.limine.enable = true;
      boot.loader.limine.efiSupport = true;
      boot.loader.limine.efiInstallAsRemovable = false;
      boot.loader.limine.settings.timeout = 0;

      environment.systemPackages = [ pkgs.efibootmgr ];
    };

  testScript = ''
    machine.start()
    machine.wait_for_unit("multi-user.target")

    with subtest("it booted, and there was no fallback path to fall back to"):
        machine.succeed("test -d /sys/firmware/efi")
        machine.succeed("test -s /boot/efi/limine/BOOT*.EFI")
        machine.fail("test -e /boot/efi/boot/BOOTX64.EFI")
        machine.fail("test -e /boot/efi/boot/BOOTAA64.EFI")

    with subtest("efibootmgr agrees the entry is well formed"):
        entries = machine.succeed("efibootmgr -v")
        print(entries)

        limine = [line for line in entries.splitlines() if "Limine" in line]
        assert len(limine) == 1, entries

        # the partition it was installed to, and the loader on it
        assert "HD(" in limine[0], limine[0]
        assert "\\efi\\limine\\" in limine[0], limine[0]

        order = [line for line in entries.splitlines() if line.startswith("BootOrder:")]
        entry_id = limine[0].split()[0].removeprefix("Boot").rstrip("*")
        assert entry_id in order[0], f"{entry_id} not in {order[0]}"

    with subtest("reinstalling reuses the entry rather than adding another"):
        machine.succeed("/run/current-system/bin/switch-to-configuration boot")

        entries = machine.succeed("efibootmgr")
        assert len([line for line in entries.splitlines() if "Limine" in line]) == 1, entries
  '';
}
