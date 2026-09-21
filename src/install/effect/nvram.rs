//! Writing the firmware boot entry.

use crate::install::{error::InstallError, error::NvramSnafu};
use efivar::{VarManager, boot::BootEntry, efi::Variable};
use snafu::ResultExt as _;

/// Write it, reusing our own slot so that its position in the boot order
/// survives.
pub(crate) fn register(entry: BootEntry) -> Result<(), InstallError> {
    let mut manager = efivar::system();

    let used = used_ids(&*manager)?;
    let id = ours(&*manager, &used, &entry.description).unwrap_or_else(|| free_id(&used));

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
fn ours(manager: &dyn VarManager, used: &[u16], label: &str) -> Option<u16> {
    used.iter().copied().find(|id| {
        let variable = Variable::new(&format!("Boot{id:04X}"));

        BootEntry::read(manager, &variable).is_ok_and(|entry| entry.description == label)
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
    use super::free_id;

    #[test]
    fn takes_the_lowest_free_slot() {
        assert_eq!(free_id(&[]), 0);
        assert_eq!(free_id(&[0, 1, 3]), 2);
        assert_eq!(free_id(&[2, 0, 1]), 3);
    }
}
