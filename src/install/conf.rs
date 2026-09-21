use super::{
    entries,
    error::{InstallError, NoGenerationsSnafu},
    facts::{Facts, Generation},
    plan::Plan,
};
use crate::config::{LimineInstallConfig, Setting};
use snafu::OptionExt as _;
use std::path::Path;

/// limine picks the entry to boot by index, and the first lines of the menu are
/// ours.
const DEFAULT_ENTRY: &str = "default_entry";

/// The one setting whose value is a path we have to put on the boot filesystem
/// ourselves.
const WALLPAPER: &str = "wallpaper";

/// The whole of limine.conf: the module's settings, then a group per profile
/// with its generations newest first, then the user's extra entries.
pub(crate) fn generate(
    plan: &mut Plan,
    cfg: &LimineInstallConfig,
    facts: &Facts,
) -> Result<String, InstallError> {
    let latest = facts.latest().context(NoGenerationsSnafu)?;

    let mut out = settings(plan, cfg, facts);

    if !cfg.settings().contains_key(DEFAULT_ENTRY) {
        out.push(format!("{DEFAULT_ENTRY}: {}\n", default_entry(latest)));
    }

    out.push("\n# NixOS boot entries start here\n".to_owned());

    let efi_support = cfg.efi_support();

    for profile in facts.profiles() {
        out.push(format!("/+NixOS {}\n", profile.group()));

        for (position, generation) in profile.generations().iter().enumerate() {
            out.push(entries::generate(
                plan,
                facts,
                generation,
                efi_support,
                position == 0,
            )?);
        }
    }

    out.push("\n# NixOS boot entries end here\n\n".to_owned());
    out.push(cfg.extra_entries().to_owned());

    Ok(out.concat().trim().to_owned())
}

/// The index of the newest generation's own entry, which is one deeper when
/// it has specialisations to hold.
fn default_entry(latest: &Generation) -> u32 {
    // see BootSpec::has_specialisations
    if latest.spec().has_specialisations() {
        3
    } else {
        2
    }
}

fn settings(plan: &mut Plan, cfg: &LimineInstallConfig, facts: &Facts) -> Vec<String> {
    let mut out = Vec::new();

    for (key, setting) in cfg.settings() {
        let values = match setting {
            Setting::One(value) => std::slice::from_ref(value),
            Setting::Many(values) => values.as_slice(),
        };

        for value in values {
            if key == WALLPAPER
                && let Some(path) = value.as_str()
            {
                let path = Path::new(path);
                let uri = plan.copied_uri(path, "wallpapers", facts.digest(path));
                out.push(format!("{key}: {uri}\n"));
            } else if let Some(value) = value.value() {
                out.push(format!("{key}: {value}\n"));
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::generate;
    use crate::{
        config::LimineInstallConfig,
        install::{facts::fixture, plan::Plan},
    };
    use serde_json::{Value, json};

    const STORE: &str = "/nix/store";

    fn boot_json(number: u32, specialisations: &str) -> String {
        format!(
            r#"{{
              "org.nixos.bootspec.v1": {{
                "init": "{STORE}/aaa-system-{number}/init",
                "initrd": "{STORE}/ccc-initrd/initrd",
                "kernel": "{STORE}/bbb-linux/Image",
                "kernelParams": ["quiet"],
                "label": "NixOS {number}",
                "system": "aarch64-linux",
                "toplevel": "{STORE}/aaa-system-{number}"
              }},
              "org.nixos.specialisation.v1": {{{specialisations}}}
            }}"#
        )
    }

    fn config(overrides: Value) -> LimineInstallConfig {
        let mut json = json!({
            "additionalFiles": {},
            "biosDevice": "nodev",
            "biosSupport": false,
            "canTouchEfiVariables": false,
            "efiMountPoint": "/boot",
            "efiRemovable": true,
            "efiSupport": true,
            "enrollConfig": false,
            "extraEntries": "",
            "fileSystems": {},
            "force": false,
            "fwupdEfiPath": null,
            "hostArchitecture": {"family": "arm", "bits": 64, "arch": "armv8-a"},
            "liminePath": "/nix/store/aaa-limine",
            "maxGenerations": 0,
            "partitionIndex": null,
            "secureBoot": {
                "enable": false, "autoGenerateKeys": false,
                "autoEnrollKeys": {"enable": false, "extraArgs": []},
                "sbctl": "/nix/store/bbb-sbctl"
            },
            "settings": {},
            "validateChecksums": false
        });

        let Value::Object(overrides) = overrides else {
            panic!("overrides must be an object");
        };

        for (key, value) in overrides {
            json[key] = value;
        }

        serde_json::from_value(json).expect("a valid config")
    }

    /// Newest generation first, which is the order limine.conf lists them in.
    fn render(overrides: Value, generations: &[u32]) -> String {
        let facts = fixture::system(
            generations
                .iter()
                .map(|n| fixture::generation(*n, &boot_json(*n, "")))
                .collect(),
        );

        let cfg = config(overrides);
        let mut plan = Plan::new(cfg.install_dir());

        generate(&mut plan, &cfg, &facts).expect("limine.conf")
    }

    /// Drop the entry bodies, which entries.rs covers, and keep the frame.
    fn skeleton(conf: &str) -> Vec<&str> {
        conf.lines()
            .filter(|line| {
                line.is_empty()
                    || line.starts_with('/')
                    || line.starts_with('#')
                    || line.starts_with("default_entry")
            })
            .collect()
    }

    #[test]
    fn frames_the_entries_with_the_markers_the_module_documents() {
        assert_eq!(
            skeleton(&render(json!({}), &[1])),
            [
                "default_entry: 2",
                "",
                "# NixOS boot entries start here",
                "/+NixOS default profile",
                "//Generation 1",
                "",
                "# NixOS boot entries end here",
            ]
        );
    }

    #[test]
    fn lists_generations_newest_first() {
        let conf = render(json!({}), &[3, 2, 1]);

        let generations: Vec<&str> = conf.lines().filter(|l| l.starts_with("//")).collect();

        assert_eq!(
            generations,
            ["//Generation 3", "//Generation 2", "//Generation 1"]
        );
    }

    #[test]
    fn gives_every_profile_its_own_group() {
        let facts = fixture::facts(vec![
            fixture::profile("system", vec![fixture::generation(1, &boot_json(1, ""))]),
            fixture::profile("test", vec![fixture::generation(1, &boot_json(1, ""))]),
        ]);

        let cfg = config(json!({}));
        let mut plan = Plan::new(cfg.install_dir());
        let conf = generate(&mut plan, &cfg, &facts).expect("conf");

        assert!(conf.contains("/+NixOS default profile"));
        assert!(conf.contains("/+NixOS profile 'test'"), "{conf}");
    }

    #[test]
    fn renders_settings_the_way_limine_spells_them() {
        let conf = render(
            json!({
                "settings": {
                    "graphics": true, "editor_enabled": false,
                    "timeout": "no", "backdrop": "2F302F"
                }
            }),
            &[1],
        );

        let settings: Vec<&str> = conf.lines().take_while(|line| !line.is_empty()).collect();

        assert_eq!(
            settings,
            [
                "backdrop: 2F302F",
                "editor_enabled: no",
                "graphics: yes",
                "timeout: no",
                "default_entry: 2",
            ]
        );
    }

    #[test]
    fn repeats_a_key_for_each_value_in_a_list() {
        let conf = render(json!({"settings": {"module_path": ["a", "b"]}}), &[1]);

        assert!(conf.contains("module_path: a\nmodule_path: b\n"), "{conf}");
    }

    /// Wallpapers are the one setting naming a file we have to put on the ESP.
    #[test]
    fn copies_wallpapers_and_refers_to_them_by_uri() {
        let facts = fixture::system(vec![fixture::generation(1, &boot_json(1, ""))]);
        let cfg = config(json!({"settings": {"wallpaper": ["/nix/store/ddd-art/bg.png"]}}));

        let mut plan = Plan::new(cfg.install_dir());
        let conf = generate(&mut plan, &cfg, &facts).expect("conf");

        assert!(
            conf.contains("wallpaper: boot():/limine/wallpapers/ddd-art-bg.png"),
            "{conf}"
        );
        assert!(
            plan.actions()
                .iter()
                .any(|action| action.destination().ends_with("wallpapers/ddd-art-bg.png")),
            "the wallpaper was never asked for"
        );
    }

    /// `default_entry` is injected only when the module has not set it, and
    /// points one deeper when the newest generation is a submenu.
    #[test]
    fn picks_a_default_entry_that_matches_the_menu_depth() {
        assert!(render(json!({}), &[1]).contains("default_entry: 2"));

        let nested = fixture::system(vec![fixture::generation(
            1,
            &boot_json(
                1,
                r#""hardened": {
                  "org.nixos.bootspec.v1": {
                    "init": "/nix/store/ddd/init", "kernel": "/nix/store/bbb-linux/Image",
                    "kernelParams": [], "label": "h", "system": "aarch64-linux",
                    "toplevel": "/nix/store/ddd"
                  },
                  "org.nixos.specialisation.v1": {}
                }"#,
            ),
        )]);

        let cfg = config(json!({}));
        let mut plan = Plan::new(cfg.install_dir());
        let conf = generate(&mut plan, &cfg, &nested).expect("conf");

        assert!(conf.contains("default_entry: 3"), "{conf}");
    }

    #[test]
    fn leaves_an_explicit_default_entry_alone() {
        let conf = render(json!({"settings": {"default_entry": 7}}), &[1]);

        assert!(conf.contains("default_entry: 7"), "{conf}");
        assert_eq!(conf.matches("default_entry").count(), 1, "{conf}");
    }

    #[test]
    fn appends_extra_entries_after_the_generated_ones() {
        let conf = render(
            json!({"extraEntries": "/memtest\n  protocol: chainload\n"}),
            &[1],
        );

        let (_, tail) = conf
            .split_once("# NixOS boot entries end here")
            .expect("marker");

        assert_eq!(tail.trim(), "/memtest\n  protocol: chainload");
    }

    /// Rendering the whole config is a function of the facts: it reads
    /// nothing and writes nothing.
    #[test]
    fn performs_no_io() {
        let facts = fixture::system(vec![fixture::generation(1, &boot_json(1, ""))]);
        let cfg = config(json!({}));
        let mut plan = Plan::new(cfg.install_dir());

        generate(&mut plan, &cfg, &facts).expect("conf");

        // /boot/limine is the install dir, and nothing may have appeared there
        assert!(!cfg.install_dir().exists());
        assert!(!plan.actions().is_empty());
    }
}
