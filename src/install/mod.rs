//! Installing limine, given a validated [`LimineInstallConfig`].

mod bios;
mod bootspec;
mod facts;

mod conf;
mod efi;
mod entries;
pub(crate) mod error;

pub(crate) mod effect;
mod nvram;
mod plan;
mod secure_boot;

use crate::config::LimineInstallConfig;
use effect::Output;
use error::{BiosInstallSnafu, InstallError};
use facts::profiles::Profiles;
use plan::Plan;
use snafu::ResultExt as _;

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
    let mut commands = Vec::new();

    if let Some(efi) = cfg.target().as_ref().here() {
        commands.extend(efi::commands(cfg, efi, &facts, &limine_conf));
    }

    // ---- from here on it is all effects ----

    effect::apply(&plan)?;

    for command in &commands {
        println!("running {}", command.program().display());
        effect::run(command)?.success()?;
    }

    // on its own, so that a failure can carry the hint that usually fixes it
    if let Some(bios) = cfg.target().as_ref().there()
        && let Some(command) = bios::command(cfg, bios)
    {
        effect::run(&command)
            .and_then(Output::success)
            .context(BiosInstallSnafu)?;
    }

    if let Some(efi) = cfg.target().as_ref().here() {
        efi::register(cfg, efi, &facts)?;
    }

    Ok(())
}
