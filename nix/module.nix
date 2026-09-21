{
  config,
  pkgs,
  lib,
  ...
}:
let
  cfg = config.boot.loader.limine;
  efi = config.boot.loader.efi;

  format = pkgs.formats.keyValue { };

  limine-install = pkgs.callPackage ./package.nix { };

  installConfig = pkgs.writeText "limine-install.json" (
    builtins.toJSON {
      inherit (cfg)
        additionalFiles
        biosDevice
        biosSupport
        efiSupport
        enrollConfig
        extraEntries
        force
        partitionIndex
        settings
        validateChecksums
        secureBoot
        ;

      liminePath = cfg.package;
      efiMountPoint = efi.efiSysMountPoint;
      canTouchEfiVariables = efi.canTouchEfiVariables;
      efiRemovable = cfg.efiInstallAsRemovable;
      fileSystems = config.fileSystems;
      maxGenerations = if cfg.maxGenerations == null then 0 else cfg.maxGenerations;
      hostArchitecture = pkgs.stdenv.hostPlatform.parsed.cpu;
      fwupdEfiPath = config.services.fwupd.package or null;
    }
  );
in
{
  # this module provides boot.loader.limine itself
  disabledModules = [ "system/boot/loader/limine/limine.nix" ];

  options.boot.loader.limine = {
    enable = lib.mkEnableOption "the limine bootloader";

    package = lib.mkPackageOption pkgs "limine" { };

    settings = lib.mkOption {
      type = lib.types.submodule {
        freeformType = format.type;

        options = {
          timeout = lib.mkOption {
            type = with lib.types; either int (enum [ "no" ]);
            default = 5;
            description = ''
              Seconds before the first entry is booted. `"no"` disables
              automatic boot, `0` boots the default entry instantly.
            '';
          };

          wallpaper = lib.mkOption {
            type = with lib.types; listOf path;
            default = [ ];
            example = lib.literalExpression "[ pkgs.nixos-artwork.wallpapers.simple-dark-gray-bootloader.gnomeFilePath ]";
            description = ''
              Wallpapers to copy to the boot filesystem. One is picked at
              random when more than one is given.
            '';
          };

          wallpaper_style = lib.mkOption {
            type = lib.types.enum [
              "centered"
              "stretched"
              "tiled"
            ];
            default = "stretched";
            description = "How the wallpaper is displayed.";
          };

          hash_mismatch_panic = lib.mkOption {
            type = lib.types.bool;
            default = false;
            description = ''
              Refuse to boot a file whose checksum does not match, rather than
              warning about it.
            '';
          };

          editor_enabled = lib.mkOption {
            type = lib.types.bool;
            default = false;
            description = ''
              Whether the boot entry editor is reachable.

              ::: {.note}
              Leaving this off is recommended: the editor can be used to gain
              root by passing `init=/bin/sh`.
              :::
            '';
          };
        };
      };
      default = { };
      description = ''
        limine.conf's global section. Anything limine understands can be set
        here; see [the upstream documentation](https://github.com/limine-bootloader/limine/blob/v${lib.versions.major cfg.package.version}.x/CONFIG.md).
      '';
    };

    maxGenerations = lib.mkOption {
      type = lib.types.nullOr lib.types.int;
      default = null;
      example = 50;
      description = ''
        How many of the newest generations to list. `null` keeps every
        generation that has not been garbage collected, which can fill a small
        boot partition.
      '';
    };

    extraEntries = lib.mkOption {
      type = lib.types.lines;
      default = "";
      example = lib.literalExpression ''
        /memtest86
          protocol: chainload
          path: boot():/efi/memtest86/memtest86.efi
      '';
      description = "Appended to the end of limine.conf.";
    };

    additionalFiles = lib.mkOption {
      type = lib.types.attrsOf lib.types.path;
      default = { };
      example = lib.literalExpression ''
        { "efi/memtest86/memtest86.efi" = "''${pkgs.memtest86-efi}/BOOTX64.efi"; }
      '';
      description = ''
        Files to copy to the EFI system partition, keyed by their destination
        relative to it.
      '';
    };

    validateChecksums = lib.mkEnableOption null // {
      default = true;
      description = "Give limine a checksum for every file it loads.";
    };

    efiSupport = lib.mkEnableOption null // {
      default = pkgs.stdenv.hostPlatform.isEfi;
      defaultText = lib.literalExpression "pkgs.stdenv.hostPlatform.isEfi";
      description = "Install limine's EFI binary.";
    };

    efiInstallAsRemovable = lib.mkEnableOption null // {
      default = !config.boot.loader.efi.canTouchEfiVariables;
      defaultText = lib.literalExpression "!config.boot.loader.efi.canTouchEfiVariables";
      description = ''
        Install to the removable media path every firmware probes, rather than
        registering a boot entry. See {option}`boot.loader.grub.efiInstallAsRemovable`.
      '';
    };

    biosSupport = lib.mkEnableOption null // {
      default = !cfg.efiSupport && pkgs.stdenv.hostPlatform.isx86;
      defaultText = lib.literalExpression "!config.boot.loader.limine.efiSupport && pkgs.stdenv.hostPlatform.isx86";
      description = "Install limine for BIOS. x86 only.";
    };

    biosDevice = lib.mkOption {
      type = lib.types.str;
      default = "nodev";
      description = ''
        Disk to write the BIOS stage 1 to. `"nodev"` installs stage 2 only.
      '';
    };

    partitionIndex = lib.mkOption {
      type = lib.types.nullOr lib.types.int;
      default = null;
      description = "1-based index of a dedicated partition for limine's stage 2.";
    };

    enrollConfig = lib.mkEnableOption null // {
      default = cfg.settings.hash_mismatch_panic;
      defaultText = lib.literalExpression "config.boot.loader.limine.settings.hash_mismatch_panic";
      description = ''
        Pin limine.conf's checksum into the EFI binary, so a tampered config
        is refused. EFI only.
      '';
    };

    force = lib.mkEnableOption null // {
      description = ''
        Install even when limine's own safety checks fail. Only if you know
        why you need it.
      '';
    };

    secureBoot = {
      enable = lib.mkEnableOption null // {
        description = ''
          Sign the limine binary with {command}`sbctl`.

          ::: {.note}
          Needs keys. See {option}`boot.loader.limine.secureBoot.autoGenerateKeys`.
          :::
        '';
      };

      autoGenerateKeys = lib.mkEnableOption null // {
        description = "Generate keys during installation when there are none.";
      };

      autoEnrollKeys = {
        enable = lib.mkEnableOption null // {
          description = "Enrol the keys that were generated.";
        };

        extraArgs = lib.mkOption {
          type = lib.types.listOf lib.types.str;
          default = [
            "--microsoft"
            "--firmware-builtin"
          ];
          description = "Extra arguments for {command}`sbctl enroll-keys`.";
        };
      };

      sbctl = lib.mkPackageOption pkgs "sbctl" { };

      databasePath = lib.mkOption {
        type = lib.types.path;
        default = "/etc/secureboot";
        description = ''
          Where sbctl keeps its keys.

          This has to match the path {option}`boot.loader.limine.secureBoot.sbctl`
          was built with, which nixpkgs sets to `/etc/secureboot` rather than
          sbctl's own `/var/lib/sbctl`. Override both together if you move it:

          ```nix
          boot.loader.limine.secureBoot = {
            sbctl = pkgs.sbctl.override { databasePath = "/var/lib/sbctl"; };
            databasePath = "/var/lib/sbctl";
          };
          ```

          The install only reads it, to decide whether keys have to be
          generated before it can sign anything.
        '';
      };
    };
  };

  config = lib.mkIf cfg.enable (
    lib.mkMerge [
      {
        assertions = [
          {
            assertion =
              pkgs.stdenv.hostPlatform.isx86_64
              || pkgs.stdenv.hostPlatform.isi686
              || pkgs.stdenv.hostPlatform.isAarch64;
            message = "limine can only be installed on x86 and aarch64";
          }
          {
            assertion = cfg.efiSupport || cfg.biosSupport;
            message = "limine needs one of efiSupport or biosSupport, or the system will not boot";
          }
        ];

        boot.loader.limine.settings = {
          graphics = true;
          wallpaper_style = lib.mkDefault "stretched";
        };

        # the toplevel is passed as the first argument; everything else the
        # install needs is in the config it is pointed at
        system.build.installBootLoader = pkgs.writeShellScript "limine-install" ''
          exec ${lib.getExe limine-install} ${installConfig}
        '';

        system.boot.loader.id = "limine";

        # the install runs the generation's append-initrd-secrets script and
        # puts the result beside the kernel as a second initrd. Saying so is
        # what stops NixOS baking those secrets into the initrd itself, where
        # they would land in the world-readable store.
        boot.loader.supportsInitrdSecrets = true;
      }

      (lib.mkIf cfg.secureBoot.enable {
        assertions = [
          {
            assertion = cfg.efiSupport;
            message = "secure boot needs an ESP to sign a binary on";
          }
          {
            assertion = cfg.enrollConfig;
            message = "leaving enrollConfig off lets secure boot be bypassed";
          }
          {
            assertion = cfg.validateChecksums;
            message = "leaving validateChecksums off lets secure boot be bypassed";
          }
          {
            assertion = cfg.settings.hash_mismatch_panic;
            message = "leaving hash_mismatch_panic off lets secure boot be bypassed";
          }
          {
            assertion = !cfg.settings.editor_enabled;
            message = "limine disables the editor under secure boot regardless";
          }
        ];
      })

      (lib.mkIf (cfg.secureBoot.enable && cfg.secureBoot.autoEnrollKeys.enable) {
        assertions = [
          {
            assertion = cfg.secureBoot.autoGenerateKeys;
            message = "autoEnrollKeys does nothing without autoGenerateKeys";
          }
        ];
      })
    ]
  );
}
