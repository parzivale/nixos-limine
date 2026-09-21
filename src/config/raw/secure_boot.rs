use crate::config::{KeyPolicy, SecureBoot};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// The module's `secureBoot` submodule.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawSecureBoot {
    pub(super) enable: bool,
    auto_generate_keys: bool,
    auto_enroll_keys: RawAutoEnrollKeys,
    sbctl: PathBuf,
    database_path: PathBuf,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAutoEnrollKeys {
    enable: bool,
    extra_args: Vec<String>,
}

/// Collapses four flags into the three states an install can be in: off; on
/// with keys that have to already exist; on with key generation, which is
/// the only path that can go on to enrol them.
///
/// The flags allow combinations that mean nothing -- enrolment without
/// generation, most obviously -- and nesting them makes those unreachable
/// rather than something every later branch has to keep checking for.
pub(super) fn resolve(r: &RawSecureBoot, fwupd: Option<&Path>) -> SecureBoot {
    if !r.enable {
        return SecureBoot::Disabled;
    }

    let keys = if r.auto_generate_keys {
        KeyPolicy::Generate {
            enroll: r
                .auto_enroll_keys
                .enable
                .then(|| r.auto_enroll_keys.extra_args.clone()),
        }
    } else {
        KeyPolicy::Require
    };

    SecureBoot::Enabled {
        sbctl: r.sbctl.join("bin/sbctl"),
        database: r.database_path.clone(),
        keys,
        fwupd: fwupd.map(Path::to_path_buf),
    }
}
