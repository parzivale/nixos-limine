use super::plan::Plan;
use crate::{
    config::{BiosConfig, LimineInstallConfig},
    util::cmd::Invocation,
};

/// Stage 2 goes next to limine.conf, where stage 1 will look for it.
pub(crate) fn plan(plan: &mut Plan, cfg: &LimineInstallConfig) {
    plan.copy(
        &cfg.bios_stage2(),
        &cfg.install_dir().join("limine-bios.sys"),
    );
}

/// Writing stage 1 to the disk, once stage 2 is there for it to find.
/// `None` when the module said `nodev`, which installs stage 2 alone.
pub(crate) fn command(cfg: &LimineInstallConfig, bios: &BiosConfig) -> Option<Invocation> {
    let stage1 = bios.stage1()?;

    let mut command = Invocation::new(cfg.limine_binary())
        .arg("bios-install")
        .arg(stage1.device());

    if let Some(index) = stage1.partition_index() {
        command = command.arg(index.to_string());
    }

    if stage1.force() {
        command = command.arg("--force");
    }

    Some(command)
}

#[cfg(test)]
mod tests {
    use super::command;
    use crate::config::LimineInstallConfig;
    use serde_json::{Value, json};

    fn config(overrides: Value) -> LimineInstallConfig {
        let mut json = json!({
            "additionalFiles": {}, "biosDevice": "nodev", "biosSupport": true,
            "canTouchEfiVariables": false, "efiMountPoint": "/boot",
            "efiRemovable": false, "efiSupport": false, "enrollConfig": false,
            "extraEntries": "", "fileSystems": {"/boot": {"fsType": "vfat"}},
            "force": false, "fwupdEfiPath": null,
            "hostArchitecture": {"family": "x86", "bits": 64, "arch": null},
            "liminePath": "/nix/store/aaa-limine", "maxGenerations": 0,
            "partitionIndex": null,
            "secureBoot": {"enable": false, "autoGenerateKeys": false,
                "autoEnrollKeys": {"enable": false, "extraArgs": []},
                "sbctl": "/nix/store/bbb-sbctl", "databasePath": "/etc/secureboot"},
            "settings": {}, "validateChecksums": false
        });

        let Value::Object(overrides) = overrides else {
            panic!("overrides must be an object");
        };

        for (key, value) in overrides {
            json[key] = value;
        }

        serde_json::from_value(json).expect("a valid config")
    }

    /// What would be run for a given biosDevice and friends.
    fn command_line(overrides: Value) -> Option<String> {
        let cfg = config(overrides);
        let bios = cfg.target().as_ref().there().expect("a bios target");

        command(&cfg, bios).map(|invocation| {
            invocation
                .line()
                .replace("/nix/store/aaa-limine/bin/limine", "limine")
        })
    }

    /// nodev installs stage 2 and writes stage 1 nowhere.
    #[test]
    fn nodev_runs_nothing() {
        assert!(command_line(json!({})).is_none());
    }

    #[test]
    fn writes_stage_1_to_the_disk_it_was_given() {
        assert_eq!(
            command_line(json!({"biosDevice": "/dev/sda"})).as_deref(),
            Some("limine bios-install /dev/sda")
        );
    }

    /// The stage 2 partition is positional, and --force comes after it.
    #[test]
    fn passes_the_stage_2_partition_and_force_in_order() {
        assert_eq!(
            command_line(json!({
                "biosDevice": "/dev/sda", "partitionIndex": 2, "force": true
            }))
            .as_deref(),
            Some("limine bios-install /dev/sda 2 --force")
        );
    }

    /// A partition index of 0 is how the module spells "not set".
    #[test]
    fn ignores_a_zero_partition_index() {
        assert_eq!(
            command_line(json!({"biosDevice": "/dev/sda", "partitionIndex": 0})).as_deref(),
            Some("limine bios-install /dev/sda")
        );
    }
}
