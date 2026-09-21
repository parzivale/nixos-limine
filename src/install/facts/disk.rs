//! Finding the partition a mount point sits on, and what the partition table
//! says about it.

use super::Partition;
use super::mountinfo;
use crate::install::error::{
    InstallError, NoSuchPartitionSnafu, PartitionTableSnafu, ReadSnafu, UnknownPartitionSnafu,
};
use snafu::{OptionExt as _, ResultExt as _};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Everything a firmware boot entry has to say about where it lives.
pub(crate) fn partition_of(mount_point: &Path) -> Result<Partition, InstallError> {
    let device = mountinfo::device_for(mount_point)?;

    let number = number(&device)?;
    let disk = disk_of(&device)?;

    let table = gpt::GptConfig::new()
        .writable(false)
        .open(&disk)
        .context(PartitionTableSnafu { path: &disk })?;

    let entry = table
        .partitions()
        .get(&number)
        .context(NoSuchPartitionSnafu { path: disk, number })?;

    Ok(Partition {
        number,
        start: entry.first_lba,
        size: entry.last_lba + 1 - entry.first_lba,
        guid: entry.part_guid,
    })
}

/// 1-based, and the same number the partition table uses.
fn number(partition: &Path) -> Result<u32, InstallError> {
    let path = sysfs_dir(partition)?.join("partition");
    let number = fs::read_to_string(&path).context(ReadSnafu { path: &path })?;

    number
        .trim()
        .parse()
        .ok()
        .context(UnknownPartitionSnafu { path: partition })
}

/// The whole disk a partition belongs to, via its parent in sysfs.
fn disk_of(partition: &Path) -> Result<PathBuf, InstallError> {
    let sysfs = sysfs_dir(partition)?;

    let disk = sysfs
        .parent()
        .and_then(Path::file_name)
        .context(UnknownPartitionSnafu { path: partition })?;

    Ok(Path::new("/dev").join(disk))
}

fn sysfs_dir(partition: &Path) -> Result<PathBuf, InstallError> {
    let partition = fs::canonicalize(partition).context(ReadSnafu { path: partition })?;
    let name = partition.strip_prefix("/dev").unwrap_or(&partition);

    let sysfs = Path::new("/sys/class/block").join(name);

    fs::canonicalize(&sysfs).context(ReadSnafu { path: sysfs })
}
