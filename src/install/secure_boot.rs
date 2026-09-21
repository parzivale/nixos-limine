//! Signing the EFI binary with sbctl.
//!
//! Deciding what sbctl has to be told is a function of the module's settings
//! and whether keys already exist; running it is the caller's job.

use super::error::{InstallError, NoSecureBootKeysSnafu};
use crate::{
    config::{KeyPolicy, SecureBoot},
    util::cmd::Invocation,
};
use snafu::{ResultExt as _, ensure};
use std::{fs, io, path::Path};

/// Where sbctl keeps the keys it generates.
pub(crate) const STATE: &str = "/var/lib/sbctl";

/// Whether sbctl already holds keys. Read once, up front, so that everything
/// below is a function of it.
pub(crate) fn keys_exist(state: &Path) -> bool {
    state.exists()
}

/// Fail before we touch the boot filesystem if the keys we were told to use
/// do not exist.
pub(crate) fn check(secure_boot: &SecureBoot, keys_exist: bool) -> Result<(), InstallError> {
    let SecureBoot::Enabled {
        keys: KeyPolicy::Require,
        ..
    } = secure_boot
    else {
        return Ok(());
    };

    ensure!(keys_exist, NoSecureBootKeysSnafu);
    Ok(())
}

/// What sbctl has to be told, in order: generate and enrol keys if that is
/// what the module asked for and there are none, then sign limine, then sign
/// fwupd's own EFI binaries so that firmware updates keep working.
pub(crate) fn commands(
    secure_boot: &SecureBoot,
    binary: &Path,
    keys_exist: bool,
    fwupd_binaries: &[std::path::PathBuf],
) -> Vec<Invocation> {
    let SecureBoot::Enabled { sbctl, keys, .. } = secure_boot else {
        return Vec::new();
    };

    let mut commands = Vec::new();

    if !keys_exist && let KeyPolicy::Generate { enroll } = keys {
        commands.push(Invocation::new(sbctl).arg("create-keys"));

        if let Some(extra) = enroll {
            commands.push(Invocation::new(sbctl).arg("enroll-keys").args(extra));
        }
    }

    commands.push(Invocation::new(sbctl).arg("sign").arg(binary));

    commands.extend(
        fwupd_binaries
            .iter()
            .map(|binary| Invocation::new(sbctl).arg("sign").arg(binary)),
    );

    commands
}

/// fwupd's EFI binaries, which are signed alongside limine's. An fwupd
/// without any is not an error.
pub(crate) fn fwupd_binaries(
    fwupd: Option<&Path>,
) -> Result<Vec<std::path::PathBuf>, InstallError> {
    let Some(fwupd) = fwupd else {
        return Ok(Vec::new());
    };

    let dir = fwupd.join("libexec/fwupd/efi");

    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context(super::error::ReadSnafu { path: dir }),
    };

    let mut binaries = Vec::new();
    for entry in entries {
        let path = entry
            .context(super::error::ReadSnafu { path: &dir })?
            .path();

        if path.extension().is_some_and(|extension| extension == "efi") {
            binaries.push(path);
        }
    }

    binaries.sort();
    Ok(binaries)
}

#[cfg(test)]
mod tests {
    use super::{check, commands};
    use crate::config::{KeyPolicy, SecureBoot};
    use std::path::{Path, PathBuf};

    const SBCTL: &str = "/nix/store/aaa-sbctl/bin/sbctl";
    const BINARY: &str = "/boot/efi/limine/BOOTAA64.EFI";

    fn enabled(keys: KeyPolicy) -> SecureBoot {
        SecureBoot::Enabled {
            sbctl: PathBuf::from(SBCTL),
            keys,
            fwupd: None,
        }
    }

    /// One line per invocation, as `program arg arg`.
    fn lines(secure_boot: &SecureBoot, keys_exist: bool, fwupd: &[PathBuf]) -> Vec<String> {
        commands(secure_boot, Path::new(BINARY), keys_exist, fwupd)
            .iter()
            .map(|invocation| format!("{invocation:?}"))
            .map(|debug| {
                debug
                    .replace(SBCTL, "sbctl")
                    .replace('"', "")
                    .replace("Invocation { program: ", "")
                    .replace(", args: [", " ")
                    .replace("] }", "")
                    .replace(',', "")
            })
            .collect()
    }

    #[test]
    fn disabled_secure_boot_runs_nothing() {
        assert!(lines(&SecureBoot::Disabled, false, &[]).is_empty());
        check(&SecureBoot::Disabled, false).expect("no check");
    }

    /// Keys we were told already exist, and do not: refuse before anything is
    /// written to the ESP.
    #[test]
    fn requiring_keys_fails_when_there_are_none() {
        let secure_boot = enabled(KeyPolicy::Require);

        let error = check(&secure_boot, false).unwrap_err();
        assert!(error.to_string().contains("no sbctl secure boot keys"));

        check(&secure_boot, true).expect("keys are there");
    }

    /// Generation is allowed to find nothing, so the check passes either way.
    #[test]
    fn generating_keys_never_fails_the_check() {
        check(&enabled(KeyPolicy::Generate { enroll: None }), false).expect("will generate");
    }

    #[test]
    fn signs_without_generating_when_keys_are_already_there() {
        let secure_boot = enabled(KeyPolicy::Generate { enroll: None });

        assert_eq!(
            lines(&secure_boot, true, &[]),
            [format!("sbctl sign {BINARY}")]
        );
    }

    #[test]
    fn generates_keys_before_signing_when_there_are_none() {
        let secure_boot = enabled(KeyPolicy::Generate { enroll: None });

        assert_eq!(
            lines(&secure_boot, false, &[]),
            [
                "sbctl create-keys".to_owned(),
                format!("sbctl sign {BINARY}")
            ]
        );
    }

    /// Enrolment only happens on the generation path, and carries the extra
    /// arguments the module set.
    #[test]
    fn enrols_generated_keys_with_the_configured_arguments() {
        let secure_boot = enabled(KeyPolicy::Generate {
            enroll: Some(vec![
                "--microsoft".to_owned(),
                "--firmware-builtin".to_owned(),
            ]),
        });

        assert_eq!(
            lines(&secure_boot, false, &[]),
            [
                "sbctl create-keys".to_owned(),
                "sbctl enroll-keys --microsoft --firmware-builtin".to_owned(),
                format!("sbctl sign {BINARY}"),
            ]
        );
    }

    /// Keys that already exist are never re-enrolled.
    #[test]
    fn does_not_enrol_over_existing_keys() {
        let secure_boot = enabled(KeyPolicy::Generate {
            enroll: Some(vec!["--microsoft".to_owned()]),
        });

        assert_eq!(
            lines(&secure_boot, true, &[]),
            [format!("sbctl sign {BINARY}")]
        );
    }

    /// fwupd's own EFI binaries have to be signed too, or firmware updates
    /// stop working under secure boot.
    #[test]
    fn signs_every_fwupd_efi_binary_after_limine() {
        let fwupd = [
            PathBuf::from("/fw/fwupd.efi"),
            PathBuf::from("/fw/fwupdx64.efi"),
        ];

        assert_eq!(
            lines(&enabled(KeyPolicy::Require), true, &fwupd),
            [
                format!("sbctl sign {BINARY}"),
                "sbctl sign /fw/fwupd.efi".to_owned(),
                "sbctl sign /fw/fwupdx64.efi".to_owned(),
            ]
        );
    }
}
