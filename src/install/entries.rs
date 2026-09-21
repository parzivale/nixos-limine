use super::{
    bootspec::{BootSpec, Xen},
    error::{InstallError, XenWithoutEfiPathSnafu},
    facts::{Facts, Generation},
    plan::Plan,
};
use snafu::OptionExt as _;
use std::path::Path;

/// Where the kernels, initrds and their secrets are copied to.
const KERNELS: &str = "kernels";

/// The menu entries for one generation: its Xen entries first, then the
/// generation itself, then its specialisations.
pub(crate) fn generate(
    plan: &mut Plan,
    facts: &Facts,
    generation: &Generation,
    efi_support: bool,
    expanded: bool,
) -> Result<String, InstallError> {
    let spec = generation.spec();
    let number = generation.number();
    let time = generation.built_at();

    let mut blocks = Vec::new();

    if let Some(xen) = spec.xen() {
        if efi_support {
            blocks.push(xen_entry(plan, facts, 2, spec, xen, number, time, true)?);
        }
        blocks.push(xen_entry(plan, facts, 2, spec, xen, number, time, false)?);
    }

    // A generation with specialisations becomes a submenu holding them.
    let depth = if spec.has_specialisations() { 3 } else { 2 };

    if spec.has_specialisations() {
        let marker = if expanded { "+" } else { "" };
        blocks.push(format!(
            "{}{marker}Generation {number}\n",
            "/".repeat(depth - 1)
        ));
        blocks.push(linux_entry(plan, facts, depth, spec, "Default", time));
    } else {
        let label = format!("Generation {number}");
        blocks.push(linux_entry(plan, facts, depth, spec, &label, time));
    }

    for (name, spec) in spec.specialisations() {
        blocks.push(linux_entry(plan, facts, depth, spec, name, time));
    }

    Ok(blocks.concat())
}

fn linux_entry(
    plan: &mut Plan,
    facts: &Facts,
    levels: usize,
    spec: &BootSpec,
    label: &str,
    time: &str,
) -> String {
    let mut lines = vec![
        format!("{}{label}", "/".repeat(levels)),
        "protocol: linux".to_owned(),
        format!("comment: {}, built on {time}", spec.label()),
        format!("kernel_path: {}", copy(plan, facts, spec.kernel(), KERNELS)),
        format!("cmdline: {}", spec.cmdline()),
    ];

    if let Some(initrd) = spec.initrd() {
        lines.push(format!(
            "module_path: {}",
            copy(plan, facts, initrd, KERNELS)
        ));
    }

    // the secrets script ran while the facts were gathered; a generation that
    // can no longer produce its secrets simply has none
    if let Some(secrets) = facts.secrets(spec.toplevel()) {
        let name = format!("{}-secrets", file_name(spec.toplevel()));

        lines.push(format!(
            "module_path: {}",
            plan.written_uri(
                &name,
                KERNELS,
                secrets.contents().to_vec(),
                secrets.digest()
            )
        ));
    }

    block(&lines)
}

/// Xen is loaded as the executable, with the kernel and initrd as modules.
/// Under EFI that goes through Xen's own EFI binary and its `.cfg`, because
/// limine cannot find an entry point in Xen's multiboot binary (limine #482),
/// and multiboot1 does not work under EFI at all (limine #483).
#[expect(clippy::too_many_arguments, reason = "one entry needs all of it")]
fn xen_entry(
    plan: &mut Plan,
    facts: &Facts,
    levels: usize,
    spec: &BootSpec,
    xen: &Xen,
    generation: u32,
    time: &str,
    efi: bool,
) -> Result<String, InstallError> {
    let version = xen.version();
    let suffix = if efi { " EFI" } else { "" };

    let mut lines = vec![
        format!(
            "{}Generation {generation} with Xen {version}{suffix}",
            "/".repeat(levels)
        ),
        format!("comment: Xen {version} {}, built on {time}", spec.label()),
    ];

    // a generation can name a multiboot binary that has since been collected
    let Some(boot) = xen.boot().filter(|boot| facts.present(boot.multiboot())) else {
        return Ok(block(&lines));
    };

    let target = format!("xen/{generation}");

    if efi {
        let binary = boot.efi().context(XenWithoutEfiPathSnafu { generation })?;

        lines.push("protocol: efi".to_owned());
        lines.push(format!(
            "path: {}",
            xen_efi_files(plan, facts, spec, xen, binary, generation)
        ));

        return Ok(block(&lines));
    }

    lines.push("protocol: multiboot".to_owned());
    lines.push(format!(
        "path: {}",
        copy(plan, facts, boot.multiboot(), &target)
    ));

    // The leading `--` works around the first argument being dropped.
    let params = xen.params();
    if !params.is_empty() {
        lines.push(format!("cmdline: -- {params}"));
    }

    lines.push(format!(
        "module_path: {}",
        copy(plan, facts, spec.kernel(), KERNELS)
    ));
    lines.push(format!("module_string: -- {}", spec.cmdline()));

    if let Some(initrd) = spec.initrd() {
        lines.push(format!(
            "module_path: {}",
            copy(plan, facts, initrd, KERNELS)
        ));
    }

    Ok(block(&lines))
}

/// Lay out the `xen/<generation>` directory that Xen's EFI binary reads, and
/// return the URI of the binary itself.
fn xen_efi_files(
    plan: &mut Plan,
    facts: &Facts,
    spec: &BootSpec,
    xen: &Xen,
    efi_path: &Path,
    generation: u32,
) -> String {
    let target = format!("xen/{generation}");

    let uri = copy(plan, facts, efi_path, &target);
    let config_path = plan.dest_path(efi_path, &target).with_extension("cfg");

    let mut lines = vec![
        format!("default=nixos{generation}"),
        String::new(),
        format!("[nixos{generation}]"),
    ];

    let params = xen.params();
    if !params.is_empty() {
        lines.push(format!("options={params}"));
    }

    let kernel = plan.dest_path(spec.kernel(), &target);
    plan.copy(spec.kernel(), &kernel);
    lines.push(format!("kernel={} {}", file_name(&kernel), spec.cmdline()));

    if let Some(initrd) = spec.initrd() {
        let dest = plan.dest_path(initrd, &target);
        plan.copy(initrd, &dest);
        lines.push(format!("ramdisk={}", file_name(&dest)));
    }

    plan.write(&config_path, block(&lines));

    uri
}

/// Ask for a file and get back the URI limine.conf names it by.
fn copy(plan: &mut Plan, facts: &Facts, path: &Path, target: &str) -> String {
    plan.copied_uri(path, target, facts.digest(path))
}

fn block(lines: &[String]) -> String {
    let mut block = lines.join("\n");
    block.push('\n');
    block
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::generate;
    use crate::install::{
        facts::{Facts, fixture},
        plan::Plan,
    };

    const TOPLEVEL: &str = "/nix/store/aaa-system";
    const KERNEL: &str = "/nix/store/bbb-linux/Image";
    const INITRD: &str = "/nix/store/ccc-initrd/initrd";
    const MULTIBOOT: &str = "/nix/store/eee-xen/xen";
    const XEN_EFI: &str = "/nix/store/eee-xen/xen.efi";

    fn boot_json(specialisations: &str, extension: &str) -> String {
        format!(
            r#"{{
              "org.nixos.bootspec.v1": {{
                "init": "{TOPLEVEL}/init",
                "initrd": "{INITRD}",
                "kernel": "{KERNEL}",
                "kernelParams": ["console=ttyAMA0", "quiet"],
                "label": "NixOS (Linux 6.18.50)",
                "system": "aarch64-linux",
                "toplevel": "{TOPLEVEL}"
              }},
              "org.nixos.specialisation.v1": {{{specialisations}}}
              {extension}
            }}"#
        )
    }

    const SPECIALISATION: &str = r#""hardened": {
      "org.nixos.bootspec.v1": {
        "init": "/nix/store/ddd-hardened/init",
        "kernel": "/nix/store/bbb-linux/Image",
        "kernelParams": ["lockdown=1"],
        "label": "NixOS hardened",
        "system": "aarch64-linux",
        "toplevel": "/nix/store/ddd-hardened"
      },
      "org.nixos.specialisation.v1": {}
    }"#;

    fn xen_extension() -> String {
        format!(
            r#", "org.xenproject.bootspec.v2": {{
              "version": "4.19",
              "params": ["dom0_mem=4G", "ucode=scan"],
              "multibootPath": "{MULTIBOOT}",
              "efiPath": "{XEN_EFI}"
            }}"#
        )
    }

    fn render(facts: &Facts, efi_support: bool, expanded: bool) -> String {
        let mut plan = Plan::new(std::path::Path::new("/boot/limine"));
        let generation = &facts.profiles()[0].generations()[0];

        generate(&mut plan, facts, generation, efi_support, expanded).expect("entries")
    }

    fn one(boot_json: &str) -> Facts {
        fixture::system(vec![fixture::generation(1, boot_json)])
    }

    #[test]
    fn renders_a_generation_as_one_entry() {
        assert_eq!(
            render(&one(&boot_json("", "")), false, false),
            "\
//Generation 1
protocol: linux
comment: NixOS (Linux 6.18.50), built on 2026-09-21 00:00:00
kernel_path: boot():/limine/kernels/bbb-linux-Image
cmdline: init=/nix/store/aaa-system/init console=ttyAMA0 quiet
module_path: boot():/limine/kernels/ccc-initrd-initrd
"
        );
    }

    /// Specialisations turn the generation into a submenu: it gains a header,
    /// its own entry becomes "Default", and everything drops a level.
    #[test]
    fn renders_specialisations_as_a_submenu() {
        let entries = render(&one(&boot_json(SPECIALISATION, "")), false, true);
        let lines: Vec<&str> = entries.lines().collect();

        assert_eq!(lines[0], "//+Generation 1");
        assert_eq!(lines[1], "///Default");
        assert!(entries.contains("\n///hardened\n"), "{entries}");
        assert!(entries.contains("cmdline: init=/nix/store/ddd-hardened/init lockdown=1"));
    }

    #[test]
    fn only_the_newest_generation_is_expanded() {
        let facts = one(&boot_json(SPECIALISATION, ""));

        assert!(render(&facts, false, true).starts_with("//+Generation 1"));
        assert!(render(&facts, false, false).starts_with("//Generation 1"));
    }

    /// With checksums on, every referenced file carries a digest limine
    /// verifies before booting it.
    #[test]
    fn appends_digests_when_checksums_are_on() {
        let facts =
            fixture::with_digests(one(&boot_json("", "")), &[(KERNEL, "aaa"), (INITRD, "bbb")]);

        let entries = render(&facts, false, false);

        assert!(entries.contains("kernel_path: boot():/limine/kernels/bbb-linux-Image#aaa"));
        assert!(entries.contains("module_path: boot():/limine/kernels/ccc-initrd-initrd#bbb"));
    }

    /// The secrets script ran while the facts were gathered; the entry just
    /// carries what it produced.
    #[test]
    fn adds_a_secrets_module_when_the_script_produced_one() {
        let facts = fixture::with_secrets(one(&boot_json("", "")), TOPLEVEL, b"secret");
        let entries = render(&facts, false, false);

        assert!(
            entries.contains("module_path: boot():/limine/kernels/aaa-system-secrets\n"),
            "{entries}"
        );
    }

    /// With checksums on, the secrets get a digest too -- taken from the
    /// bytes, since they were never a file in the store to read back.
    #[test]
    fn checksums_the_secrets_it_was_handed() {
        let mut facts = fixture::with_secrets(one(&boot_json("", "")), TOPLEVEL, b"secret");
        facts = fixture::with_secrets_digest(facts, TOPLEVEL, "deadbeef");

        assert!(
            render(&facts, false, false)
                .contains("module_path: boot():/limine/kernels/aaa-system-secrets#deadbeef"),
            "{}",
            render(&facts, false, false)
        );
    }

    /// An older generation that can no longer produce its secrets simply gets
    /// no secrets module, rather than failing the install.
    #[test]
    fn omits_the_secrets_module_when_the_script_produced_nothing() {
        let entries = render(&one(&boot_json("", "")), false, false);

        assert!(!entries.contains("secrets"), "{entries}");
    }

    /// Xen loads as the executable with the kernel and initrd as modules. The
    /// leading `--` on both cmdlines works around limine dropping the first
    /// argument.
    #[test]
    fn renders_a_xen_multiboot_entry() {
        let facts = fixture::with_present(one(&boot_json("", &xen_extension())), &[MULTIBOOT]);

        let entries = render(&facts, false, false);
        let xen: Vec<&str> = entries
            .lines()
            .take_while(|line| *line != "//Generation 1")
            .collect();

        assert_eq!(
            xen,
            [
                "//Generation 1 with Xen 4.19",
                "comment: Xen 4.19 NixOS (Linux 6.18.50), built on 2026-09-21 00:00:00",
                "protocol: multiboot",
                "path: boot():/limine/xen/1/eee-xen-xen",
                "cmdline: -- dom0_mem=4G ucode=scan",
                "module_path: boot():/limine/kernels/bbb-linux-Image",
                "module_string: -- init=/nix/store/aaa-system/init console=ttyAMA0 quiet",
                "module_path: boot():/limine/kernels/ccc-initrd-initrd",
            ]
        );
    }

    /// Under EFI, limine chainloads Xen's own EFI binary instead, because it
    /// cannot find an entry point in the multiboot one (limine #482).
    #[test]
    fn renders_a_xen_efi_entry_when_efi_is_supported() {
        let facts = fixture::with_present(one(&boot_json("", &xen_extension())), &[MULTIBOOT]);

        let entries = render(&facts, true, false);

        assert!(
            entries.contains("//Generation 1 with Xen 4.19 EFI\n"),
            "{entries}"
        );
        assert!(entries.contains("protocol: efi\npath: boot():/limine/xen/1/eee-xen-xen.efi"));
        assert!(entries.contains("protocol: multiboot"));
    }

    /// Xen's EFI binary reads this beside itself, so the kernel and initrd go
    /// into the same directory under their own names.
    #[test]
    fn writes_the_xen_efi_config_beside_its_binary() {
        let facts = fixture::with_present(one(&boot_json("", &xen_extension())), &[MULTIBOOT]);

        let mut plan = Plan::new(std::path::Path::new("/boot/limine"));
        let generation = &facts.profiles()[0].generations()[0];
        generate(&mut plan, &facts, generation, true, false).expect("entries");

        let written = plan
            .actions()
            .iter()
            .find_map(|action| match action {
                crate::install::plan::Action::Write { to, contents }
                    if to.ends_with("eee-xen-xen.cfg") =>
                {
                    Some(contents.clone())
                }
                _ => None,
            })
            .expect("a xen.cfg");

        assert_eq!(
            String::from_utf8(written).expect("utf8"),
            "default=nixos1\n\n[nixos1]\noptions=dom0_mem=4G ucode=scan\n\
             kernel=bbb-linux-Image init=/nix/store/aaa-system/init console=ttyAMA0 quiet\n\
             ramdisk=ccc-initrd-initrd\n"
        );
    }

    /// A multiboot binary that has been garbage collected leaves the entry
    /// listed but with no protocol to boot it by.
    #[test]
    fn a_xen_entry_whose_multiboot_binary_is_gone_carries_no_protocol() {
        let entries = render(&one(&boot_json("", &xen_extension())), false, false);

        assert!(
            entries.contains("//Generation 1 with Xen 4.19\n"),
            "{entries}"
        );
        assert!(!entries.contains("protocol: multiboot"), "{entries}");
        assert!(!entries.contains("protocol: efi"), "{entries}");
    }

    /// Rendering asks for the kernel and initrd but must not copy them, and
    /// must not read anything either: none of these paths exist.
    #[test]
    fn performs_no_io() {
        let facts = one(&boot_json("", ""));
        let mut plan = Plan::new(std::path::Path::new("/boot/limine"));
        let generation = &facts.profiles()[0].generations()[0];

        generate(&mut plan, &facts, generation, false, false).expect("entries");

        assert_eq!(plan.actions().len(), 2);
    }
}
