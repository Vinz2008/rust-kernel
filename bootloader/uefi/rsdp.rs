use uefi::system;
use uefi::table::cfg::ConfigTableEntry;

pub fn find_rsdp() -> Option<u64> {
    system::with_config_table(|tables| {
        // Prefer ACPI 2.0+
        if let Some(entry) = tables.iter().find(|entry| entry.guid == ConfigTableEntry::ACPI2_GUID){
            return Some(entry.address as u64);
        }

        // Fall back to ACPI 1.0
        tables.iter().find(|entry| entry.guid == ConfigTableEntry::ACPI_GUID).map(|entry| entry.address as u64)
    })
}