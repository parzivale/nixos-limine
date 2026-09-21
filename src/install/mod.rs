//! Installing limine, given a validated [`LimineInstallConfig`].

mod bios;
mod bootspec;
mod facts;

mod conf;
mod efi;
mod entries;
pub(crate) mod error;

mod nvram;
mod plan;
mod profiles;
mod secure_boot;

use crate::config::LimineInstallConfig;
use crate::util::cmd::Output;
use error::{BiosInstallSnafu, InstallError, ReadSnafu, SyncFsSnafu};
use plan::Plan;
use profiles::Profiles;
use snafu::ResultExt as _;
use std::{fs::File, path::Path};

pub(crate) fn run(cfg: &LimineInstallConfig) -> Result<(), InstallError> {
    // everything the install reads from the world, read once
    let facts = facts::gather(cfg, &Profiles::system())?;

    if let Some(secure_boot) = cfg.secure_boot() {
        secure_boot::check(secure_boot, facts.sbctl_keys_exist())?;
    }

    let mut plan = Plan::new(cfg.install_dir());

    let limine_conf = conf::generate(&mut plan, cfg, &facts)?;
    plan.write(&cfg.limine_conf(), limine_conf.clone());

    for (dest, source) in cfg.additional_files() {
        plan.copy(source, &cfg.mount_point().join(dest));
    }

    if let Some(efi) = cfg.target().as_ref().here() {
        efi::plan(&mut plan, cfg, efi);
    }

    if cfg.target().as_ref().there().is_some() {
        bios::plan(&mut plan, cfg);
    }

    // what has to be run once those files are in place. worked out before
    // anything is written, so a run that cannot succeed says so first.
    let fwupd = secure_boot::fwupd_binaries(cfg.fwupd())?;
    let mut commands = Vec::new();

    if let Some(efi) = cfg.target().as_ref().here() {
        commands.extend(efi::commands(cfg, efi, &facts, &limine_conf, &fwupd));
    }

    // ---- from here on it is all effects ----

    plan.apply()?;

    for command in &commands {
        println!("running {}", command.program().display());
        command.run()?.success()?;
    }

    // on its own, so that a failure can carry the hint that usually fixes it
    if let Some(bios) = cfg.target().as_ref().there()
        && let Some(command) = bios::command(cfg, bios)
    {
        command
            .run()
            .and_then(Output::success)
            .context(BiosInstallSnafu)?;
    }

    if let Some(efi) = cfg.target().as_ref().here() {
        efi::register(cfg, efi, &facts)?;
    }

    Ok(())
}

/// fat32 offers little in the way of recovery after a crash, and an outage
/// shortly after an update can leave the system unbootable. Flush the boot
/// filesystem whether or not the install got that far.
pub(crate) fn sync(mount_point: &Path) -> Result<(), InstallError> {
    let dir = File::open(mount_point).context(ReadSnafu { path: mount_point })?;

    rustix::fs::syncfs(&dir).context(SyncFsSnafu { path: mount_point })
}
