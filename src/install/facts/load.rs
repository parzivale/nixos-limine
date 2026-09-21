//! Reading the world into [`Facts`].
//!
//! Everything here is an effect. Once it has run, working out what to install
//! is a pure function of the config and what this found.

use super::disk;
use super::{Facts, Generation, Partition, Profile, SYSTEM};
use crate::install::{
    bootspec::BootSpec,
    error::{InstallError, ReadSnafu},
    profiles::Profiles,
    secure_boot,
};
use crate::{
    config::{LimineInstallConfig, Setting},
    util::{cmd, hash},
};
use rustix::{fs::Mode, process};
use snafu::ResultExt as _;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    os::unix::fs::MetadataExt as _,
    path::{Path, PathBuf},
    process::Command,
};
/// Read the world.
pub(crate) fn gather(
    cfg: &LimineInstallConfig,
    profiles: &Profiles,
) -> Result<Facts, InstallError> {
    let profiles = read_profiles(cfg, profiles)?;
    let secrets = run_secrets_scripts(&profiles)?;

    let referenced = referenced(cfg, &profiles);

    Ok(Facts {
        present: referenced
            .iter()
            .filter(|path| path.exists())
            .cloned()
            .collect(),
        digests: digests(cfg, &referenced)?,
        esp: esp(cfg)?,
        sbctl_keys_exist: secure_boot::keys_exist(Path::new(secure_boot::STATE)),
        profiles,
        secrets,
    })
}

/// Every profile and the generations it still has, newest first.
fn read_profiles(
    cfg: &LimineInstallConfig,
    profiles: &Profiles,
) -> Result<Vec<Profile>, InstallError> {
    let mut names = vec![SYSTEM.to_owned()];
    names.extend(profiles.list()?);

    names
        .into_iter()
        .map(|name| {
            let mut generations = profiles.generations(&name, cfg.max_generations())?;
            generations.reverse();

            let generations = generations
                .into_iter()
                .map(|number| read_generation(profiles, &name, number))
                .collect::<Result<_, _>>()?;

            Ok(Profile { name, generations })
        })
        .collect()
}

fn read_generation(
    profiles: &Profiles,
    profile: &str,
    number: u32,
) -> Result<Generation, InstallError> {
    let link = profiles.generation_path(profile, number);

    Ok(Generation {
        number,
        built_at: built_at(&link)?,
        spec: BootSpec::load(&link.join("boot.json"))?,
    })
}

/// Every file limine.conf will name. Which those are is decided by the
/// generations we just read, so this can run ahead of the rendering.
fn referenced(cfg: &LimineInstallConfig, profiles: &[Profile]) -> BTreeSet<PathBuf> {
    let mut paths = BTreeSet::new();

    for profile in profiles {
        for generation in &profile.generations {
            collect_referenced(generation.spec(), &mut paths);
        }
    }

    if let Some(Setting::Many(wallpapers)) = cfg.settings().get("wallpaper") {
        paths.extend(
            wallpapers
                .iter()
                .filter_map(|w| w.as_str())
                .map(PathBuf::from),
        );
    }

    paths
}

/// The digest of every file limine.conf will name.
fn digests(
    cfg: &LimineInstallConfig,
    referenced: &BTreeSet<PathBuf>,
) -> Result<BTreeMap<PathBuf, String>, InstallError> {
    if !cfg.validate_checksums() {
        return Ok(BTreeMap::new());
    }

    referenced
        .iter()
        .map(|path| {
            let digest = hash::blake2b_file(path).context(ReadSnafu { path })?;
            Ok((path.clone(), digest))
        })
        .collect()
}

fn collect_referenced(spec: &BootSpec, paths: &mut BTreeSet<PathBuf>) {
    paths.insert(spec.kernel().to_path_buf());
    paths.extend(spec.initrd().map(Path::to_path_buf));

    if let Some(xen) = spec.xen()
        && let Some(boot) = xen.boot()
    {
        paths.insert(boot.multiboot().to_path_buf());
        paths.extend(boot.efi().map(Path::to_path_buf));
    }

    for spec in spec.specialisations().values() {
        collect_referenced(spec, paths);
    }
}

/// Generations carrying an initrd secrets script have to have it run before
/// we can know whether their entry gets a secrets module.
///
/// A failure is not fatal: older generations routinely no longer have the
/// secrets they were built with.
fn run_secrets_scripts(profiles: &[Profile]) -> Result<BTreeMap<PathBuf, Vec<u8>>, InstallError> {
    let mut secrets = BTreeMap::new();

    for profile in profiles {
        for generation in &profile.generations {
            let spec = generation.spec();

            let Some(script) = spec.initrd_secrets() else {
                continue;
            };

            if let Some(built) = run_secrets_script(script, spec.toplevel())? {
                secrets.insert(spec.toplevel().to_path_buf(), built);
            }
        }
    }

    Ok(secrets)
}

fn run_secrets_script(script: &Path, toplevel: &Path) -> Result<Option<Vec<u8>>, InstallError> {
    let name = toplevel.file_name().unwrap_or_default().to_string_lossy();
    let tmp = std::env::temp_dir().join(format!("{}-{name}-secrets", std::process::id()));

    let previous = process::umask(Mode::from_bits_truncate(0o137));
    let outcome = cmd::run(Command::new(script).arg(&tmp));
    process::umask(previous);

    let failure = match outcome {
        Ok(output) if output.status().success() => None,
        Ok(output) => Some(output.text().to_owned()),
        Err(error) => Some(error.to_string()),
    };

    if let Some(reason) = failure {
        eprintln!(
            "warning: failed to create initrd secrets for {}: {}",
            toplevel.display(),
            reason.trim()
        );
        println!("note: if this is an older generation there is nothing to worry about");
    }

    if !tmp.exists() {
        return Ok(None);
    }

    // read it out and drop it rather than leaving secrets in /tmp
    let contents = fs::read(&tmp).context(ReadSnafu { path: &tmp })?;
    fs::remove_file(&tmp).context(ReadSnafu { path: &tmp })?;

    Ok(Some(contents))
}

/// Only needed when we are going to register a boot entry.
fn esp(cfg: &LimineInstallConfig) -> Result<Option<Partition>, InstallError> {
    if !cfg.registers_boot_entry() {
        return Ok(None);
    }

    disk::partition_of(cfg.mount_point()).map(Some)
}

fn built_at(link: &Path) -> Result<String, InstallError> {
    let mtime = fs::symlink_metadata(link)
        .context(ReadSnafu { path: link })?
        .mtime();

    let stamp = jiff::Timestamp::from_second(mtime).unwrap_or(jiff::Timestamp::UNIX_EPOCH);

    Ok(stamp
        .to_zoned(jiff::tz::TimeZone::system())
        .strftime("%F %H:%M:%S")
        .to_string())
}
