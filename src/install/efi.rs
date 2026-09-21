use super::{
    error::{InstallError, NoEspSnafu},
    facts::Facts,
    nvram,
    plan::Plan,
    secure_boot,
};
use crate::{
    config::{EfiDiscovery, EfiInstall, LimineInstallConfig},
    util::{cmd::Invocation, hash},
};
use snafu::OptionExt as _;

/// Put limine's EFI binary where the firmware will find it.
pub(crate) fn plan(plan: &mut Plan, cfg: &LimineInstallConfig, efi: &EfiInstall) {
    let dest = efi.image_path(cfg.mount_point(), cfg.arch().efi_boot_file());

    plan.copy(&cfg.efi_image(), &dest);
}

/// What has to be run once that binary is on the ESP: pinning the config to
/// it, and signing it.
pub(crate) fn commands(
    cfg: &LimineInstallConfig,
    efi: &EfiInstall,
    facts: &Facts,
    limine_conf: &str,
) -> Vec<Invocation> {
    let binary = efi.image_path(cfg.mount_point(), cfg.arch().efi_boot_file());

    let mut commands = Vec::new();

    if efi.enroll_config() {
        commands.push(
            Invocation::new(cfg.limine_binary())
                .arg("enroll-config")
                .arg(&binary)
                .arg(hash::blake2b(limine_conf.as_bytes())),
        );
    }

    commands.extend(secure_boot::commands(
        efi.secure_boot(),
        &binary,
        facts.sbctl_keys_exist(),
        facts.fwupd_binaries(),
    ));

    commands
}

/// Making the firmware aware of limine, once it is in place.
pub(crate) fn register(
    cfg: &LimineInstallConfig,
    efi: &EfiInstall,
    facts: &Facts,
) -> Result<(), InstallError> {
    match efi.discovery() {
        EfiDiscovery::Removable => Ok(()),
        EfiDiscovery::Registered => {
            let esp = facts.esp().context(NoEspSnafu)?;

            super::effect::register(nvram::entry(esp, cfg.arch().efi_boot_file()))
        }
        EfiDiscovery::Unregistered => {
            eprintln!(
                "warning: both boot.loader.efi.canTouchEfiVariables and boot.loader.limine.efiInstallAsRemovable are false,\n  so limine was installed to /efi/limine without an EFI boot entry. This may render the system unbootable."
            );

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{commands, plan};
    use crate::{
        config::LimineInstallConfig,
        install::{facts::fixture, plan::Plan},
        util::hash,
    };
    use serde_json::{Value, json};
    use std::path::PathBuf;

    const LIMINE: &str = "/nix/store/aaa-limine";
    const SBCTL: &str = "/nix/store/bbb-sbctl";

    fn config(overrides: Value) -> LimineInstallConfig {
        let mut json = json!({
            "additionalFiles": {},
            "biosDevice": "nodev",
            "biosSupport": false,
            "canTouchEfiVariables": false,
            "efiMountPoint": "/boot",
            "efiRemovable": true,
            "efiSupport": true,
            "enrollConfig": false,
            "extraEntries": "",
            "fileSystems": {},
            "force": false,
            "fwupdEfiPath": null,
            "hostArchitecture": {"family": "arm", "bits": 64, "arch": "armv8-a"},
            "liminePath": LIMINE,
            "maxGenerations": 0,
            "partitionIndex": null,
            "secureBoot": {
                "enable": false, "autoGenerateKeys": false,
                "autoEnrollKeys": {"enable": false, "extraArgs": []},
                "sbctl": SBCTL, "databasePath": "/etc/secureboot"
            },
            "settings": {},
            "validateChecksums": false
        });

        let Value::Object(overrides) = overrides else {
            panic!("overrides must be an object");
        };

        for (key, value) in overrides {
            json[key] = value;
        }

        serde_json::from_value(json).expect("a valid config")
    }

    /// One line per invocation, with the store paths shortened.
    fn lines(overrides: Value, keys_exist: bool, fwupd: &[PathBuf]) -> Vec<String> {
        let cfg = config(overrides);
        let efi = cfg.target().as_ref().here().expect("an efi target");

        let mut facts = fixture::system(vec![]);
        if keys_exist {
            facts = fixture::with_sbctl_keys(facts);
        }
        facts = fixture::with_fwupd(facts, fwupd);

        commands(&cfg, efi, &facts, "timeout: 5\n")
            .iter()
            .map(|invocation| {
                invocation
                    .line()
                    .replace(&format!("{LIMINE}/bin/limine"), "limine")
                    .replace(&format!("{SBCTL}/bin/sbctl"), "sbctl")
            })
            .collect()
    }

    /// A removable install goes to the path every firmware probes; a
    /// registered one goes under its own directory.
    #[test]
    fn copies_the_image_to_wherever_the_firmware_will_look() {
        for (removable, dest) in [
            (true, "/boot/efi/boot/BOOTAA64.EFI"),
            (false, "/boot/efi/limine/BOOTAA64.EFI"),
        ] {
            let cfg = config(json!({"efiRemovable": removable}));
            let efi = cfg.target().as_ref().here().expect("an efi target");

            let mut p = Plan::new(cfg.install_dir());
            plan(&mut p, &cfg, efi);

            let action = &p.actions()[0];
            assert_eq!(action.destination(), PathBuf::from(dest));
        }
    }

    /// Planning the copy records it rather than making it.
    #[test]
    fn planning_the_image_only_records_it() {
        let cfg = config(json!({}));
        let efi = cfg.target().as_ref().here().expect("an efi target");

        let mut p = Plan::new(cfg.install_dir());
        plan(&mut p, &cfg, efi);

        assert_eq!(p.actions().len(), 1);
    }

    #[test]
    fn a_plain_install_runs_nothing_afterwards() {
        assert!(lines(json!({}), false, &[]).is_empty());
    }

    /// The digest pinned into the binary is of the config we just rendered,
    /// so a tampered limine.conf is refused at boot.
    #[test]
    fn enrolling_the_config_pins_its_digest() {
        assert_eq!(
            lines(json!({"enrollConfig": true}), false, &[]),
            [format!(
                "limine enroll-config /boot/efi/boot/BOOTAA64.EFI {}",
                hash::blake2b(b"timeout: 5\n")
            )]
        );
    }

    /// Signing comes after enrolment, since both act on the same binary and
    /// the signature has to cover the enrolled digest.
    #[test]
    fn signs_after_enrolling() {
        let lines = lines(
            json!({
                "enrollConfig": true,
                "secureBoot": {
                    "enable": true, "autoGenerateKeys": false,
                    "autoEnrollKeys": {"enable": false, "extraArgs": []},
                    "sbctl": SBCTL, "databasePath": "/etc/secureboot"
                }
            }),
            true,
            &[],
        );

        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].starts_with("limine enroll-config"), "{lines:?}");
        assert_eq!(lines[1], "sbctl sign /boot/efi/boot/BOOTAA64.EFI");
    }

    /// A registered install signs the binary where it actually put it.
    #[test]
    fn signs_the_binary_it_installed() {
        let lines = lines(
            json!({
                "efiRemovable": false,
                "secureBoot": {
                    "enable": true, "autoGenerateKeys": false,
                    "autoEnrollKeys": {"enable": false, "extraArgs": []},
                    "sbctl": SBCTL, "databasePath": "/etc/secureboot"
                }
            }),
            true,
            &[],
        );

        assert_eq!(lines, ["sbctl sign /boot/efi/limine/BOOTAA64.EFI"]);
    }

    /// fwupd's binaries are signed alongside ours.
    #[test]
    fn passes_fwupd_binaries_through_to_sbctl() {
        let lines = lines(
            json!({
                "secureBoot": {
                    "enable": true, "autoGenerateKeys": false,
                    "autoEnrollKeys": {"enable": false, "extraArgs": []},
                    "sbctl": SBCTL, "databasePath": "/etc/secureboot"
                }
            }),
            true,
            &[PathBuf::from("/fw/fwupd.efi")],
        );

        assert_eq!(
            lines,
            [
                "sbctl sign /boot/efi/boot/BOOTAA64.EFI",
                "sbctl sign /fw/fwupd.efi",
            ]
        );
    }
}
