use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// What the install intends to put on the boot filesystem.
///
/// Building one touches nothing. [`crate::install::effect::apply`] is what
/// carries it out.
pub(crate) struct Plan {
    install_dir: PathBuf,
    actions: Vec<Action>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// Make the destination match the source.
    Copy { from: PathBuf, to: PathBuf },
    /// Copy only if the destination is missing. Copied files are named after
    /// the store path they came from, so one that is already there is already
    /// the right bytes -- and kernels are big.
    CopyIfMissing { from: PathBuf, to: PathBuf },
    /// Write freshly generated contents.
    Write { to: PathBuf, contents: Vec<u8> },
}

impl Action {
    pub(crate) fn destination(&self) -> &Path {
        match self {
            Self::Copy { to, .. } | Self::CopyIfMissing { to, .. } | Self::Write { to, .. } => to,
        }
    }
}

impl Plan {
    pub(crate) fn new(install_dir: &Path) -> Self {
        Self {
            install_dir: install_dir.to_path_buf(),
            actions: Vec::new(),
        }
    }

    pub(crate) fn install_dir(&self) -> &Path {
        &self.install_dir
    }

    pub(crate) fn actions(&self) -> &[Action] {
        &self.actions
    }

    pub(crate) fn copy(&mut self, from: &Path, to: &Path) {
        self.actions.push(Action::Copy {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
        });
    }

    /// For files whose name already carries their store hash, so one that is
    /// there is the right one.
    pub(crate) fn copy_if_missing(&mut self, from: &Path, to: &Path) {
        self.actions.push(Action::CopyIfMissing {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
        });
    }

    pub(crate) fn write(&mut self, to: &Path, contents: impl Into<Vec<u8>>) {
        self.actions.push(Action::Write {
            to: to.to_path_buf(),
            contents: contents.into(),
        });
    }

    /// Where `path` lands under `target`, named after the store path it came
    /// from so that two generations never collide.
    pub(crate) fn dest_path(&self, path: &Path, target: &str) -> PathBuf {
        self.install_dir().join(target).join(dest_file(path))
    }

    /// Ask for `path` under `target`, and return the URI limine.conf refers to
    /// it by. The digest is one limine verifies before booting the file, and
    /// is absent when checksums are off.
    pub(crate) fn copied_uri(&mut self, path: &Path, target: &str, digest: Option<&str>) -> String {
        let to = self.dest_path(path, target);

        self.copy_if_missing(path, &to);

        let mut uri = format!("boot():{}", uri_path(path, target).display());

        if let Some(digest) = digest {
            uri.push('#');
            uri.push_str(digest);
        }

        uri
    }
}
/// The `boot():`-relative path of a file copied into `target`.
fn uri_path(path: &Path, target: &str) -> PathBuf {
    Path::new("/limine").join(target).join(dest_file(path))
}

fn dest_file(path: &Path) -> OsString {
    let package = path.parent().and_then(Path::file_name).unwrap_or_default();

    let mut name = package.to_os_string();
    name.push("-");
    name.push(path.file_name().unwrap_or_default());
    name
}

#[cfg(test)]
mod tests {
    use super::{Action, Plan, dest_file, uri_path};
    use std::path::{Path, PathBuf};

    const KERNEL: &str = "/nix/store/abcdef-linux-6.18.50/Image";

    fn plan() -> Plan {
        Plan::new(Path::new("/boot/limine"))
    }

    /// The store hash has to survive into the name, or two generations sharing
    /// a filename would overwrite each other on the ESP.
    #[test]
    fn names_copies_after_the_store_path_they_came_from() {
        assert_eq!(dest_file(Path::new(KERNEL)), "abcdef-linux-6.18.50-Image");
    }

    #[test]
    fn puts_copies_under_their_target_directory() {
        assert_eq!(
            plan().dest_path(Path::new(KERNEL), "kernels"),
            PathBuf::from("/boot/limine/kernels/abcdef-linux-6.18.50-Image")
        );
    }

    /// limine resolves `boot():` against the volume it was loaded from, so the
    /// URI is rooted at /limine regardless of where the ESP is mounted.
    #[test]
    fn uris_are_relative_to_the_boot_volume() {
        assert_eq!(
            uri_path(Path::new(KERNEL), "kernels"),
            PathBuf::from("/limine/kernels/abcdef-linux-6.18.50-Image")
        );
    }

    #[test]
    fn an_empty_target_puts_the_file_at_the_top() {
        assert_eq!(
            uri_path(Path::new(KERNEL), ""),
            PathBuf::from("/limine/abcdef-linux-6.18.50-Image")
        );
    }

    /// Building the plan records intent; `Plan` has no way to carry it out.
    #[test]
    fn records_what_it_would_do_without_doing_it() {
        let mut plan = plan();
        plan.copy(
            Path::new("/nix/store/x-limine/BOOTAA64.EFI"),
            Path::new("/boot/efi/boot/BOOTAA64.EFI"),
        );
        plan.write(Path::new("/boot/limine/limine.conf"), "timeout: 5\n");

        assert_eq!(
            plan.actions(),
            [
                Action::Copy {
                    from: PathBuf::from("/nix/store/x-limine/BOOTAA64.EFI"),
                    to: PathBuf::from("/boot/efi/boot/BOOTAA64.EFI"),
                },
                Action::Write {
                    to: PathBuf::from("/boot/limine/limine.conf"),
                    contents: b"timeout: 5\n".to_vec(),
                },
            ]
        );
    }

    /// Kernels are content-addressed by name, so re-copying one is only ever
    /// wasted I/O -- and they are the big files.
    #[test]
    fn asks_for_kernels_only_if_they_are_missing() {
        let mut plan = plan();
        let uri = plan.copied_uri(Path::new(KERNEL), "kernels", None);

        assert_eq!(uri, "boot():/limine/kernels/abcdef-linux-6.18.50-Image");
        assert!(matches!(plan.actions(), [Action::CopyIfMissing { .. }]));
    }

    /// limine verifies the digest before booting the file.
    #[test]
    fn appends_a_digest_when_it_is_given_one() {
        let mut plan = plan();
        let uri = plan.copied_uri(Path::new(KERNEL), "kernels", Some("abc123"));

        assert_eq!(
            uri,
            "boot():/limine/kernels/abcdef-linux-6.18.50-Image#abc123"
        );
    }
}
