use crate::install::bootspec::{Xen, XenBoot};
use serde::Deserialize;
use std::path::PathBuf;

/// `org.xenproject.bootspec.v2`. Every field is optional: the extension has
/// grown over time, and a generation may carry only part of it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawXen {
    efi_path: Option<PathBuf>,
    multiboot_path: Option<PathBuf>,
    #[serde(default)]
    params: Vec<String>,
    version: Option<String>,
}

/// `None` when the extension names no version.
///
/// The version is what the menu entries are labelled with, so a generation
/// whose extension predates it -- or carries only part of it -- gets no Xen
/// entries rather than ones that cannot be told apart.
///
/// Whether the multiboot binary is still on disk is not asked here: that is a
/// fact about the world, and is gathered with the others.
pub(super) fn resolve(r: RawXen) -> Option<Xen> {
    Some(Xen {
        version: r.version?,
        params: r.params,
        boot: r
            .multiboot_path
            .filter(|path| !path.as_os_str().is_empty())
            .map(|multiboot| XenBoot {
                multiboot,
                efi: r.efi_path,
            }),
    })
}
