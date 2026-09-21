//! Everything the install needs to know about the world, read once so that
//! deciding what to do is a function from data to data.
//!
//! Gathering these is the only part of working out an install that touches the
//! filesystem or runs anything; see [`gather`].

mod disk;
mod load;
pub(crate) mod mountinfo;
pub(crate) mod profiles;

pub(crate) use load::gather;

use super::bootspec::BootSpec;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// What the machine looked like when we started.
#[derive(Debug, Default)]
pub(crate) struct Facts {
    profiles: Vec<Profile>,
    digests: BTreeMap<PathBuf, String>,
    /// fwupd's EFI binaries, which are signed alongside ours.
    pub(super) fwupd: Vec<PathBuf>,
    /// Referenced files that are actually still on disk. A generation can
    /// name one that has since been garbage collected.
    present: BTreeSet<PathBuf>,
    secrets: BTreeMap<Toplevel, Vec<u8>>,
    sbctl_keys_exist: bool,
    esp: Option<Partition>,
}

/// A generation's toplevel, which is what its secrets are keyed by.
pub(crate) type Toplevel = PathBuf;

#[derive(Debug)]
pub(crate) struct Profile {
    name: String,
    /// Newest first, already trimmed to `maxGenerations`.
    generations: Vec<Generation>,
}

#[derive(Debug)]
pub(crate) struct Generation {
    number: u32,
    spec: BootSpec,
    /// The profile link's mtime, rendered the way limine.conf wants it.
    built_at: String,
}

/// The partition the ESP lives on, as its firmware boot entry needs to name it.
#[derive(Debug, Clone)]
pub(crate) struct Partition {
    number: u32,
    start: u64,
    size: u64,
    guid: uuid::Uuid,
}

impl Facts {
    pub(crate) fn profiles(&self) -> &[Profile] {
        &self.profiles
    }

    /// `None` when checksums are off, or for a file nothing referenced.
    pub(crate) fn digest(&self, path: &Path) -> Option<&str> {
        self.digests.get(path).map(String::as_str)
    }

    /// Whether a file a generation names is still there.
    pub(crate) fn present(&self, path: &Path) -> bool {
        self.present.contains(path)
    }

    /// The secrets a generation's script produced, if it produced any.
    pub(crate) fn secrets(&self, toplevel: &Path) -> Option<&[u8]> {
        self.secrets.get(toplevel).map(Vec::as_slice)
    }

    pub(crate) fn fwupd_binaries(&self) -> &[PathBuf] {
        &self.fwupd
    }

    pub(crate) const fn sbctl_keys_exist(&self) -> bool {
        self.sbctl_keys_exist
    }

    pub(crate) const fn esp(&self) -> Option<&Partition> {
        self.esp.as_ref()
    }

    /// The newest generation of the system profile, which the default entry
    /// points at.
    pub(crate) fn latest(&self) -> Option<&Generation> {
        self.profiles
            .iter()
            .find(|profile| profile.name == SYSTEM)
            .and_then(|profile| profile.generations.first())
    }
}

/// The profile every machine has.
pub(crate) const SYSTEM: &str = "system";

impl Profile {
    pub(crate) fn generations(&self) -> &[Generation] {
        &self.generations
    }

    /// How the group is labelled in the menu.
    pub(crate) fn group(&self) -> String {
        if self.name == SYSTEM {
            "default profile".to_owned()
        } else {
            format!("profile '{}'", self.name)
        }
    }
}

impl Generation {
    pub(crate) const fn number(&self) -> u32 {
        self.number
    }

    pub(crate) const fn spec(&self) -> &BootSpec {
        &self.spec
    }

    pub(crate) fn built_at(&self) -> &str {
        &self.built_at
    }
}

impl Partition {
    pub(crate) const fn number(&self) -> u32 {
        self.number
    }

    pub(crate) const fn start(&self) -> u64 {
        self.start
    }

    pub(crate) const fn size(&self) -> u64 {
        self.size
    }

    pub(crate) const fn guid(&self) -> uuid::Uuid {
        self.guid
    }
}

/// Builders for facts that were never read off a disk.
#[cfg(test)]
pub(crate) mod fixture {
    use super::{Facts, Generation, Partition, Profile, SYSTEM};
    use crate::install::bootspec::BootSpec;
    use std::{collections::BTreeMap, path::PathBuf};

    /// A generation whose bootspec is `boot_json`, built at a fixed time so
    /// that rendered entries can be compared verbatim.
    pub(crate) fn generation(number: u32, boot_json: &str) -> Generation {
        Generation {
            number,
            spec: serde_json::from_str::<BootSpec>(boot_json).expect("valid boot.json"),
            built_at: "2026-09-21 00:00:00".to_owned(),
        }
    }

    pub(crate) fn profile(name: &str, generations: Vec<Generation>) -> Profile {
        Profile {
            name: name.to_owned(),
            generations,
        }
    }

    /// The system profile and nothing else.
    pub(crate) fn system(generations: Vec<Generation>) -> Facts {
        facts(vec![profile(SYSTEM, generations)])
    }

    pub(crate) fn facts(profiles: Vec<Profile>) -> Facts {
        Facts {
            profiles,
            ..Facts::default()
        }
    }

    /// The partition an ESP might sit on.
    pub(crate) const fn esp() -> Partition {
        Partition {
            number: 1,
            start: 2048,
            size: 1_048_576,
            guid: uuid::uuid!("1c06f03b-704e-4657-b9cd-681a087a2fdc"),
        }
    }

    /// Pretend fwupd has these EFI binaries.
    pub(crate) fn with_fwupd(mut facts: Facts, binaries: &[PathBuf]) -> Facts {
        facts.fwupd = binaries.to_vec();
        facts
    }

    /// Pretend sbctl already holds keys.
    pub(crate) const fn with_sbctl_keys(mut facts: Facts) -> Facts {
        facts.sbctl_keys_exist = true;
        facts
    }

    /// Pretend these files are still on disk.
    pub(crate) fn with_present(mut facts: Facts, paths: &[&str]) -> Facts {
        facts.present = paths.iter().map(PathBuf::from).collect();
        facts
    }

    /// Pretend checksums are on, with a digest per path.
    pub(crate) fn with_digests(mut facts: Facts, digests: &[(&str, &str)]) -> Facts {
        facts.digests = digests
            .iter()
            .map(|(path, digest)| (PathBuf::from(path), (*digest).to_owned()))
            .collect();

        facts
    }

    /// Pretend a generation's secrets script produced something.
    pub(crate) fn with_secrets(mut facts: Facts, toplevel: &str, contents: &[u8]) -> Facts {
        facts.secrets = BTreeMap::from([(PathBuf::from(toplevel), contents.to_vec())]);
        facts
    }
}
