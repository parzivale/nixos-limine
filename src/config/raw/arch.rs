use crate::config::{
    Arch,
    error::{ConfigError, UnsupportedArchSnafu},
};
use serde::Deserialize;

/// `pkgs.stdenv.hostPlatform.parsed.cpu`. Wide enough to describe CPUs limine
/// has never heard of, hence the narrowing below.
#[derive(Deserialize)]
pub(super) struct RawArch {
    family: String,
    bits: u32,
    arch: Option<String>,
}

/// Which limine binaries this CPU needs, or a hard error.
///
/// The wire format describes a CPU as a family, a width and an optional
/// architecture name, which between them can describe far more than limine
/// ships binaries for. Narrowing here means an unsupported CPU is refused
/// before the install has written anything, rather than when it reaches for
/// a file that was never built.
pub(super) fn resolve(r: &RawArch) -> Result<Arch, ConfigError> {
    Ok(match (r.family.as_str(), r.bits, r.arch.as_deref()) {
        ("x86", 32, _) => Arch::I686,
        ("x86", 64, _) => Arch::X86_64,
        ("arm", 64, Some("armv8-a")) => Arch::Aarch64,
        _ => {
            return UnsupportedArchSnafu {
                family: r.family.clone(),
                bits: r.bits,
                arch: r.arch.clone(),
            }
            .fail();
        }
    })
}
