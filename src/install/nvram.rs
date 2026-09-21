//! The boot entry that makes a firmware find limine.
//!
//! This only builds it. Writing it to the firmware's variables is
//! [`crate::install::effect::register`].
//!
//! These bytes decide whether the machine boots, and a wrong one fails at the
//! next power-on rather than here, so the device path is worth being careful
//! with. `efivar` is a weak source for it: `format` is an untyped u8 and
//! `EFIHardDriveType::Unknown::as_u8` panics outright. `uefi` (uefi-rs)
//! generates its device path nodes from the UEFI spec and does build off a
//! UEFI target, so it would be the better source -- but it carries no
//! `EFI_LOAD_OPTION` and no efivarfs, which is the rest of what this needs.

use super::facts::Partition;
use efivar::boot::{
    BootEntry, BootEntryAttributes, EFIHardDrive, EFIHardDriveType, FilePath, FilePathList,
};

/// What the entry is called in the firmware's boot menu.
const LABEL: &str = "Limine";

/// The GPT-flavoured `MBRType` of a hard drive device path node.
const GPT_FORMAT: u8 = 0x02;

/// The boot entry naming limine on the partition it was installed to.
pub(crate) fn entry(esp: &Partition, boot_file: &str) -> BootEntry {
    BootEntry {
        attributes: BootEntryAttributes::LOAD_OPTION_ACTIVE,
        description: LABEL.to_owned(),
        file_path_list: Some(FilePathList {
            file_path: FilePath {
                path: format!("\\efi\\limine\\{boot_file}"),
            },
            hard_drive: EFIHardDrive {
                partition_number: esp.number(),
                partition_start: esp.start(),
                partition_size: esp.size(),
                partition_sig: esp.guid(),
                format: GPT_FORMAT,
                sig_type: EFIHardDriveType::Gpt,
            },
        }),
        optional_data: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{GPT_FORMAT, entry};
    use crate::install::facts::fixture::esp;
    use efivar::boot::{BootEntryAttributes, EFIHardDriveType};

    /// The bytes here are what decides whether the firmware finds limine at
    /// all, and a wrong one fails at the next reboot rather than now.
    #[test]
    fn names_the_partition_the_esp_is_on() {
        let entry = entry(&esp(), "BOOTAA64.EFI");

        let list = entry.file_path_list.expect("a device path");
        let drive = list.hard_drive;

        assert_eq!(drive.partition_number, 1);
        assert_eq!(drive.partition_start, 2048);
        assert_eq!(drive.partition_size, 1_048_576);
        assert_eq!(drive.partition_sig, esp().guid());
        assert_eq!(drive.format, GPT_FORMAT);
        assert_eq!(drive.sig_type, EFIHardDriveType::Gpt);
    }

    /// Backslashes, and the directory a registered install uses rather than
    /// the removable one.
    #[test]
    fn points_at_the_loader_the_way_efi_spells_paths() {
        let entry = entry(&esp(), "BOOTX64.EFI");

        assert_eq!(
            entry.file_path_list.expect("a device path").file_path.path,
            r"\efi\limine\BOOTX64.EFI"
        );
    }

    #[test]
    fn is_an_active_entry_the_firmware_will_try() {
        let entry = entry(&esp(), "BOOTAA64.EFI");

        assert_eq!(entry.description, "Limine");
        assert!(
            entry
                .attributes
                .contains(BootEntryAttributes::LOAD_OPTION_ACTIVE)
        );
    }
}
