//! Configuración de tablas de páginas x86-64 (Paging de 4 niveles con páginas Huge de 2MB).

use capsule_core::types::GuestAddress;
use capsule_memory::GuestMemory;

pub const PML4_ADDRESS: GuestAddress = GuestAddress(0x9000);
pub const PDPT_ADDRESS: GuestAddress = GuestAddress(0xA000);
pub const PD_ADDRESS: GuestAddress = GuestAddress(0xB000);

// Flags de tabla de páginas x86-64:
// Bit 0: Present (1)
// Bit 1: Writable (1)
// Bit 2: User-accessible (1)
// Bit 7: Page Size (1 = Huge page de 2MB en nivel PD)
const PAGE_PRESENT_WRITABLE: u64 = 0x03;
const PAGE_HUGE_2MB: u64 = 0x83;

/// Configura tablas de páginas con mapeo de identidad (identity-mapping) en memoria guest
/// cubriendo los primeros 1 GB de RAM en páginas de 2MB.
pub fn setup_identity_pagetables(mem: &mut GuestMemory) -> Result<GuestAddress, String> {
    // 1. Limpiar las páginas para PML4, PDPT y PD (4096 bytes cada una)
    let zero_page = [0u8; 4096];
    mem.write_bytes(PML4_ADDRESS, &zero_page)
        .map_err(|e| format!("Error limpiando PML4: {}", e))?;
    mem.write_bytes(PDPT_ADDRESS, &zero_page)
        .map_err(|e| format!("Error limpiando PDPT: {}", e))?;
    mem.write_bytes(PD_ADDRESS, &zero_page)
        .map_err(|e| format!("Error limpiando PD: {}", e))?;

    // 2. PML4: Entrada 0 apunta a PDPT en 0xA000
    let pml4_entry = PDPT_ADDRESS.raw() | PAGE_PRESENT_WRITABLE;
    mem.write_bytes(PML4_ADDRESS, &pml4_entry.to_le_bytes())
        .map_err(|e| format!("Error escribiendo entrada PML4: {}", e))?;

    // 3. PDPT: Entrada 0 apunta a PD en 0xB000
    let pdpt_entry = PD_ADDRESS.raw() | PAGE_PRESENT_WRITABLE;
    mem.write_bytes(PDPT_ADDRESS, &pdpt_entry.to_le_bytes())
        .map_err(|e| format!("Error escribiendo entrada PDPT: {}", e))?;

    // 4. PD: 512 entradas de 2MB cada una (mapeando de 0 a 1 GB)
    for i in 0..512u64 {
        let phys_addr = i * 2 * 1024 * 1024;
        let pd_entry = phys_addr | PAGE_HUGE_2MB;
        let entry_gpa = GuestAddress::new(PD_ADDRESS.raw() + (i * 8));
        mem.write_bytes(entry_gpa, &pd_entry.to_le_bytes())
            .map_err(|e| format!("Error escribiendo entrada PD {}: {}", i, e))?;
    }

    Ok(PML4_ADDRESS)
}
