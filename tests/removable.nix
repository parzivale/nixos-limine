# limine installed to the removable media path, which every firmware probes
# without needing an NVRAM entry.
{ module }:
{
  name = "limine-removable";

  nodes.machine =
    { pkgs, ... }:
    {
      imports = [ module ];

      # build a real disk image and boot it through the bootloader, rather
      # than letting qemu load the kernel directly
      virtualisation.useBootLoader = true;
      virtualisation.useEFIBoot = true;

      boot.loader.grub.enable = false;

      boot.loader.limine.enable = true;
      boot.loader.limine.efiSupport = true;
      boot.loader.limine.efiInstallAsRemovable = true;
      boot.loader.limine.settings.timeout = 0;

      # kept out of the store and appended to the initrd at install time, so
      # the installer has to run the generation's script and hand limine the
      # result as a second initrd
      boot.initrd.secrets."/etc/limine-test-secret" = pkgs.writeText "secret" "hunter2";

      environment.systemPackages = [ pkgs.efibootmgr ];
    };

  testScript = ''
    machine.start()
    machine.wait_for_unit("multi-user.target")

    with subtest("the system that came up is the one limine was given"):
        # nothing was passed on the qemu command line, so everything running
        # here was loaded off the ESP by limine
        machine.succeed("test -d /sys/firmware/efi")
        machine.succeed("grep -q init=/nix/store/ /proc/cmdline")

    with subtest("the ESP holds what the installer put there"):
        conf = machine.succeed("cat /boot/limine/limine.conf")
        print(conf)

        assert "timeout: 0" in conf
        assert "# NixOS boot entries start here" in conf
        assert "/+NixOS default profile" in conf
        assert "protocol: linux" in conf

        machine.succeed("test -s /boot/efi/boot/BOOT*.EFI")
        machine.succeed("ls /boot/limine/kernels/*-Image /boot/limine/kernels/*-initrd")

    with subtest("every file it refers to carries a checksum"):
        machine.succeed(
            "grep -E '^kernel_path: boot\\(\\):/limine/kernels/.*#[0-9a-f]{128}$'"
            " /boot/limine/limine.conf"
        )

    with subtest("the initrd secrets were built and handed to limine"):
        conf = machine.succeed("cat /boot/limine/limine.conf")

        # the initrd, then the secrets appended to it
        modules = [line for line in conf.splitlines() if line.startswith("module_path:")]
        assert len(modules) == 2, conf

        # a uri is boot():<path>#<digest>, and only the path is a filename
        named = modules[1].split("/")[-1].split("#")[0]
        assert named.endswith("-secrets"), modules

        # exactly one: key material has no business being copied twice
        secrets = machine.succeed("ls /boot/limine/kernels/ | grep -- -secrets").split()
        assert secrets == [named], (secrets, named)

    with subtest("no boot entry was registered"):
        # efiInstallAsRemovable means the firmware finds it by path alone
        machine.fail("efibootmgr | grep -q Limine")

    with subtest("reinstalling over the top changes nothing"):
        before = machine.succeed("sha256sum /boot/limine/limine.conf")
        machine.succeed("/run/current-system/bin/switch-to-configuration boot")
        after = machine.succeed("sha256sum /boot/limine/limine.conf")

        assert before == after, f"{before!r} != {after!r}"
  '';
}
