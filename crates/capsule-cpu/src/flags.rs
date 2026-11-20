//! Banderas arquitecturales de RFLAGS para x86-64.

pub const FLAG_CF: u64 = 1 << 0;  // Carry Flag
pub const FLAG_RESERVED: u64 = 1 << 1; // Siempre 1 en x86
pub const FLAG_PF: u64 = 1 << 2;  // Parity Flag
pub const FLAG_AF: u64 = 1 << 4;  // Auxiliary Carry Flag
pub const FLAG_ZF: u64 = 1 << 6;  // Zero Flag
pub const FLAG_SF: u64 = 1 << 7;  // Sign Flag
pub const FLAG_TF: u64 = 1 << 8;  // Trap Flag
pub const FLAG_IF: u64 = 1 << 9;  // Interrupt Enable Flag
pub const FLAG_DF: u64 = 1 << 10; // Direction Flag
pub const FLAG_OF: u64 = 1 << 11; // Overflow Flag

/// Calcula la bandera de paridad (PF): activa si el byte inferior tiene un número par de bits en 1.
#[inline]
pub fn calculate_parity(val: u8) -> bool {
    val.count_ones() % 2 == 0
}

/// Calcula las banderas para suma de 32 bits.
pub fn update_add_flags_u32(rflags: &mut u64, op1: u32, op2: u32, res: u32) {
    let mut flags = *rflags & !(FLAG_CF | FLAG_PF | FLAG_AF | FLAG_ZF | FLAG_SF | FLAG_OF);

    if (res as u64) < (op1 as u64) {
        flags |= FLAG_CF;
    }
    if res == 0 {
        flags |= FLAG_ZF;
    }
    if (res & 0x80000000) != 0 {
        flags |= FLAG_SF;
    }
    if calculate_parity((res & 0xFF) as u8) {
        flags |= FLAG_PF;
    }
    // Overflow: (op1 ^ res) & (op2 ^ res) bit 31
    if ((op1 ^ res) & (op2 ^ res) & 0x80000000) != 0 {
        flags |= FLAG_OF;
    }

    *rflags = flags;
}

/// Calcula las banderas para suma de 64 bits.
pub fn update_add_flags_u64(rflags: &mut u64, op1: u64, op2: u64, res: u64) {
    let mut flags = *rflags & !(FLAG_CF | FLAG_PF | FLAG_AF | FLAG_ZF | FLAG_SF | FLAG_OF);

    if res < op1 {
        flags |= FLAG_CF;
    }
    if res == 0 {
        flags |= FLAG_ZF;
    }
    if (res & 0x8000000000000000) != 0 {
        flags |= FLAG_SF;
    }
    if calculate_parity((res & 0xFF) as u8) {
        flags |= FLAG_PF;
    }
    if ((op1 ^ res) & (op2 ^ res) & 0x8000000000000000) != 0 {
        flags |= FLAG_OF;
    }

    *rflags = flags;
}

/// Calcula las banderas para resta o comparación (CMP / SUB) de 32 bits: op1 - op2 = res.
pub fn update_sub_flags_u32(rflags: &mut u64, op1: u32, op2: u32, res: u32) {
    let mut flags = *rflags & !(FLAG_CF | FLAG_PF | FLAG_AF | FLAG_ZF | FLAG_SF | FLAG_OF);

    if op1 < op2 {
        flags |= FLAG_CF;
    }
    if res == 0 {
        flags |= FLAG_ZF;
    }
    if (res & 0x80000000) != 0 {
        flags |= FLAG_SF;
    }
    if calculate_parity((res & 0xFF) as u8) {
        flags |= FLAG_PF;
    }
    // Overflow: (op1 ^ op2) & (op1 ^ res) bit 31
    if ((op1 ^ op2) & (op1 ^ res) & 0x80000000) != 0 {
        flags |= FLAG_OF;
    }

    *rflags = flags;
}

/// Calcula las banderas para resta o comparación (CMP / SUB) de 64 bits: op1 - op2 = res.
pub fn update_sub_flags_u64(rflags: &mut u64, op1: u64, op2: u64, res: u64) {
    let mut flags = *rflags & !(FLAG_CF | FLAG_PF | FLAG_AF | FLAG_ZF | FLAG_SF | FLAG_OF);

    if op1 < op2 {
        flags |= FLAG_CF;
    }
    if res == 0 {
        flags |= FLAG_ZF;
    }
    if (res & 0x8000000000000000) != 0 {
        flags |= FLAG_SF;
    }
    if calculate_parity((res & 0xFF) as u8) {
        flags |= FLAG_PF;
    }
    if ((op1 ^ op2) & (op1 ^ res) & 0x8000000000000000) != 0 {
        flags |= FLAG_OF;
    }

    *rflags = flags;
}

/// Calcula las banderas para operaciones lógicas (AND, OR, XOR, TEST).
/// En x86, las operaciones lógicas siempre limpian CF y OF.
pub fn update_logic_flags_u64(rflags: &mut u64, res: u64, is_64bit: bool) {
    let mut flags = *rflags & !(FLAG_CF | FLAG_OF | FLAG_ZF | FLAG_SF | FLAG_PF);

    if is_64bit {
        if res == 0 {
            flags |= FLAG_ZF;
        }
        if (res & 0x8000000000000000) != 0 {
            flags |= FLAG_SF;
        }
    } else {
        let res32 = res as u32;
        if res32 == 0 {
            flags |= FLAG_ZF;
        }
        if (res32 & 0x80000000) != 0 {
            flags |= FLAG_SF;
        }
    }

    if calculate_parity((res & 0xFF) as u8) {
        flags |= FLAG_PF;
    }

    *rflags = flags;
}
