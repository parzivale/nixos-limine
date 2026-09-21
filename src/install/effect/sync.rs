//! Flushing the boot filesystem.

use crate::install::error::{InstallError, ReadSnafu, SyncFsSnafu};
use snafu::ResultExt as _;
use std::{fs::File, path::Path};

/// fat32 offers little in the way of recovery after a crash, and an outage
/// shortly after an update can leave the system unbootable. Flush the boot
/// filesystem whether or not the install got that far.
pub(crate) fn sync(mount_point: &Path) -> Result<(), InstallError> {
    let dir = File::open(mount_point).context(ReadSnafu { path: mount_point })?;

    rustix::fs::syncfs(&dir).context(SyncFsSnafu { path: mount_point })
}
