//! Registering limine with the firmware.
//!
//! Building the entry is a function of the partition we were told about;
//! [`register`] is the only part that touches the firmware's variables.
//!
//! A move off `efivar` is warranted. Its device path handling is hand-rolled
//! -- `format` is a bare u8, and `EFIHardDriveType::Unknown::as_u8` panics --
//! and the crate has been quiet since early 2024. `uefi` (uefi-rs) generates
//! its device path nodes from the spec, types the partition format and
//! signature, and does build and run outside a UEFI target, so it is the
//! better source for the bytes that decide whether the firmware finds us.
//!
//! What it does not carry is `EFI_LOAD_OPTION` or efivarfs access, so the swap
//! costs us the `Boot####` payload assembly and a small efivarfs writer
//! (rustix, which we already depend on, has the ioctls for the immutable flag
//! efivar clears today).

use super::{error::InstallError, error::NvramSnafu, facts::Partition};
use efivar::{
    VarManager,
    boot::{
        BootEntry, BootEntryAttributes, EFIHardDrive, EFIHardDriveType, FilePath, FilePathList,
    },
    efi::Variable,
};
use snafu::ResultExt as _;

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

/// Write it, reusing our own slot so that its position in the boot order
/// survives.
pub(crate) fn register(entry: BootEntry) -> Result<(), InstallError> {
    let mut manager = efivar::system();

    let used = used_ids(&*manager)?;
    let id = ours(&*manager, &used).unwrap_or_else(|| free_id(&used));

    manager.add_boot_entry(id, entry).context(NvramSnafu)?;

    // a missing BootOrder is not an error: it means nothing is registered yet.
    let mut order = manager.get_boot_order().unwrap_or_default();
    if !order.contains(&id) {
        order.insert(0, id);
        manager.set_boot_order(order).context(NvramSnafu)?;
    }

    Ok(())
}

/// The id of the entry we wrote last time, if it is still there. Looked up by
/// label, over every Boot#### there is rather than only those in the boot
/// order, so that an entry the firmware has dropped from `BootOrder` is still
/// reused rather than duplicated.
fn ours(manager: &dyn VarManager, used: &[u16]) -> Option<u16> {
    used.iter().copied().find(|id| {
        let variable = Variable::new(&format!("Boot{id:04X}"));

        BootEntry::read(manager, &variable).is_ok_and(|entry| entry.description == LABEL)
    })
}

fn used_ids(manager: &dyn VarManager) -> Result<Vec<u16>, InstallError> {
    let variables = manager.get_all_vars().context(NvramSnafu)?;

    Ok(variables
        .filter_map(|variable| variable.boot_var_id())
        .collect())
}

fn free_id(used: &[u16]) -> u16 {
    (0..=u16::MAX)
        .find(|id| !used.contains(id))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{GPT_FORMAT, entry, free_id};
    use crate::install::facts::fixture::esp;
    use efivar::boot::{BootEntryAttributes, EFIHardDriveType};

    #[test]
    fn takes_the_lowest_free_slot() {
        assert_eq!(free_id(&[]), 0);
        assert_eq!(free_id(&[0, 1, 3]), 2);
        assert_eq!(free_id(&[2, 0, 1]), 3);
    }

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
