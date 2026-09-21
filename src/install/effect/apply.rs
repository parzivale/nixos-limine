//! Carrying out a [`Plan`]: the copies, the writes, and the prune.

use crate::install::{
    error::{CopySnafu, CreateDirSnafu, InstallError, ReadSnafu, RemoveSnafu, WriteSnafu},
    plan::{Action, Plan},
};
use snafu::ResultExt as _;
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::Write as _,
    path::{Path, PathBuf},
};

/// Everything the plan asks for, then everything under the install directory
/// that it does not.
pub(crate) fn apply(plan: &Plan) -> Result<(), InstallError> {
    for action in plan.actions() {
        match action {
            Action::Copy { from, to } => copy(from, to)?,
            Action::CopyIfMissing { from, to } => {
                if !to.exists() {
                    copy(from, to)?;
                }
            }
            Action::Write { to, contents } => write(to, contents)?,
        }
    }

    prune(plan)
}

fn prune(plan: &Plan) -> Result<(), InstallError> {
    let wanted: BTreeSet<&Path> = plan.actions().iter().map(Action::destination).collect();

    let mut present = Vec::new();
    collect(plan.install_dir(), &mut present)?;

    for path in present
        .iter()
        .filter(|path| !wanted.contains(path.as_path()))
    {
        fs::remove_file(path).context(RemoveSnafu { path })?;
    }

    // a generation that has gone away takes its xen directory with it
    remove_empty_dirs(plan.install_dir())
}

fn with_tmp_suffix(path: &Path) -> PathBuf {
    let mut tmp = path.to_path_buf().into_os_string();
    tmp.push(".tmp");
    tmp.into()
}
fn copy(from: &Path, to: &Path) -> Result<(), InstallError> {
    create_parent(to)?;

    let tmp = with_tmp_suffix(to);
    fs::copy(from, &tmp).context(CopySnafu { from, to: &tmp })?;
    fs::rename(&tmp, to).context(CopySnafu { from: &tmp, to })
}

/// Replace a file, making sure it has actually hit the disk before the old
/// contents go away.
fn write(to: &Path, contents: &[u8]) -> Result<(), InstallError> {
    create_parent(to)?;

    let tmp = with_tmp_suffix(to);
    let mut file = File::create(&tmp).context(WriteSnafu { path: &tmp })?;
    file.write_all(contents)
        .and_then(|()| file.sync_all())
        .context(WriteSnafu { path: &tmp })?;

    fs::rename(&tmp, to).context(WriteSnafu { path: to })
}

fn create_parent(path: &Path) -> Result<(), InstallError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };

    if !parent.exists() {
        fs::create_dir_all(parent).context(CreateDirSnafu { path: parent })?;
    }

    Ok(())
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), InstallError> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(dir).context(ReadSnafu { path: dir })? {
        let path = entry.context(ReadSnafu { path: dir })?.path();

        if path.is_dir() {
            collect(&path, files)?;
        } else {
            files.push(path);
        }
    }

    Ok(())
}

/// Depth first, so that a directory emptied by its children goes too.
fn remove_empty_dirs(dir: &Path) -> Result<(), InstallError> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(dir).context(ReadSnafu { path: dir })? {
        let path = entry.context(ReadSnafu { path: dir })?.path();

        if path.is_dir() {
            remove_empty_dirs(&path)?;

            if fs::read_dir(&path)
                .context(ReadSnafu { path: &path })?
                .next()
                .is_none()
            {
                fs::remove_dir(&path).context(RemoveSnafu { path })?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::apply;
    use crate::install::plan::Plan;
    use std::{
        fs,
        path::{Path, PathBuf},
    };
    use tempfile::TempDir;

    struct Fixture {
        dir: TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                dir: TempDir::new().expect("temp dir"),
            }
        }

        fn install_dir(&self) -> PathBuf {
            self.dir.path().join("boot/limine")
        }

        fn plan(&self) -> Plan {
            Plan::new(&self.install_dir())
        }

        fn source(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.dir.path().join(name);
            fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            fs::write(&path, contents).expect("write");
            path
        }

        /// A file already sitting under the install directory.
        fn existing(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.install_dir().join(name);
            fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            fs::write(&path, contents).expect("write");
            path
        }
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).expect("read")
    }

    #[test]
    fn creates_the_directories_it_needs() {
        let fixture = Fixture::new();
        let source = fixture.source("store/kernel", "bytes");
        let dest = fixture.install_dir().join("kernels/deep/kernel");

        let mut plan = fixture.plan();
        plan.copy(&source, &dest);
        apply(&plan).expect("apply");

        assert_eq!(read(&dest), "bytes");
    }

    #[test]
    fn a_copy_replaces_whatever_was_there() {
        let fixture = Fixture::new();
        let source = fixture.source("store/efi", "new");
        let dest = fixture.existing("BOOTAA64.EFI", "old");

        let mut plan = fixture.plan();
        plan.copy(&source, &dest);
        apply(&plan).expect("apply");

        assert_eq!(read(&dest), "new");
    }

    /// The whole point of the distinction: a kernel already on the ESP is
    /// the same kernel, and copying 60M again is pure waste.
    #[test]
    fn copy_if_missing_leaves_an_existing_file_alone() {
        let fixture = Fixture::new();
        let source = fixture.source("store/kernel", "new");
        let dest = fixture.existing("kernels/kernel", "old");

        let mut plan = fixture.plan();
        plan.copy_if_missing(&source, &dest);
        apply(&plan).expect("apply");

        assert_eq!(read(&dest), "old");
    }

    #[test]
    fn copy_if_missing_still_copies_when_it_is_not_there() {
        let fixture = Fixture::new();
        let source = fixture.source("store/kernel", "new");
        let dest = fixture.install_dir().join("kernels/kernel");

        let mut plan = fixture.plan();
        plan.copy_if_missing(&source, &dest);
        apply(&plan).expect("apply");

        assert_eq!(read(&dest), "new");
    }

    #[test]
    fn writes_generated_contents() {
        let fixture = Fixture::new();
        let dest = fixture.install_dir().join("limine.conf");

        let mut plan = fixture.plan();
        plan.write(&dest, "timeout: 5\n");
        apply(&plan).expect("apply");

        assert_eq!(read(&dest), "timeout: 5\n");
    }

    /// Anything under the install directory the plan did not ask for
    /// belongs to a generation that has gone away.
    #[test]
    fn prunes_what_the_plan_did_not_ask_for() {
        let fixture = Fixture::new();
        let source = fixture.source("store/kernel", "bytes");
        let kept = fixture.install_dir().join("kernels/kernel");
        let stale = fixture.existing("kernels/old-kernel", "stale");

        let mut plan = fixture.plan();
        plan.copy(&source, &kept);
        apply(&plan).expect("apply");

        assert!(kept.exists());
        assert!(!stale.exists());
    }

    /// The EFI binary and additionalFiles land outside the install
    /// directory, and must survive a prune that never surveyed them.
    #[test]
    fn never_touches_anything_outside_the_install_directory() {
        let fixture = Fixture::new();
        let source = fixture.source("store/efi", "bytes");
        let outside = fixture.dir.path().join("boot/efi/boot/BOOTAA64.EFI");
        let bystander = fixture.source("boot/unrelated", "keep me");

        let mut plan = fixture.plan();
        plan.copy(&source, &outside);
        apply(&plan).expect("apply");

        assert!(outside.exists());
        assert!(bystander.exists());
    }

    /// A generation that has gone away takes its xen directory with it.
    #[test]
    fn prunes_directories_left_empty() {
        let fixture = Fixture::new();
        fixture.existing("xen/220/xen.cfg", "stale");

        apply(&fixture.plan()).expect("apply");

        assert!(!fixture.install_dir().join("xen/220").exists());
        assert!(!fixture.install_dir().join("xen").exists());
    }

    /// Applying the same plan twice has to leave the same result.
    #[test]
    fn is_idempotent() {
        let fixture = Fixture::new();
        let source = fixture.source("store/kernel", "bytes");
        let dest = fixture.install_dir().join("kernels/kernel");

        let mut plan = fixture.plan();
        plan.copy_if_missing(&source, &dest);

        apply(&plan).expect("first");
        apply(&plan).expect("second");

        assert_eq!(read(&dest), "bytes");
    }
}
