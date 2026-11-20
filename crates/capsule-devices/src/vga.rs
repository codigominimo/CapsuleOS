pub const VBE_DISPI_IOPORT_INDEX: u16 = 0x01CE;
pub const VBE_DISPI_IOPORT_DATA: u16 = 0x01CF;

pub const VBE_DISPI_INDEX_ID: u16 = 0x0;
pub const VBE_DISPI_INDEX_XRES: u16 = 0x1;
pub const VBE_DISPI_INDEX_YRES: u16 = 0x2;
pub const VBE_DISPI_INDEX_BPP: u16 = 0x3;
pub const VBE_DISPI_INDEX_ENABLE: u16 = 0x4;
pub const VBE_DISPI_INDEX_BANK: u16 = 0x5;
pub const VBE_DISPI_INDEX_VIRT_WIDTH: u16 = 0x6;
pub const VBE_DISPI_INDEX_VIRT_HEIGHT: u16 = 0x7;
pub const VBE_DISPI_INDEX_X_OFFSET: u16 = 0x8;
pub const VBE_DISPI_INDEX_Y_OFFSET: u16 = 0x9;
pub const VBE_DISPI_INDEX_VIDEO_MEMORY_64K: u16 = 0xA;

pub const VBE_DISPI_ID0: u16 = 0xB0C0;
pub const VBE_DISPI_ID1: u16 = 0xB0C1;
pub const VBE_DISPI_ID2: u16 = 0xB0C2;
pub const VBE_DISPI_ID3: u16 = 0xB0C3;
pub const VBE_DISPI_ID4: u16 = 0xB0C4;
pub const VBE_DISPI_ID5: u16 = 0xB0C5;

pub const VBE_DISPI_DISABLED: u16 = 0x00;
pub const VBE_DISPI_ENABLED: u16 = 0x01;
pub const VBE_DISPI_GETCAPS: u16 = 0x02;
pub const VBE_DISPI_LFB_ENABLED: u16 = 0x40;
pub const VBE_DISPI_NOCLEARMEM: u16 = 0x80;

/// Dirección física base del framebuffer lineal (LFB) en memoria guest.
/// Se ubica en RAM guest (4 MiB) para compatibilidad con el modelo actual sin MMIO.
pub const VBE_DISPI_LFB_PHYSICAL_ADDRESS: u64 = 0x0040_0000;

/// Memoria de video anunciada (en bloques de 64 KiB). 8 MiB.
pub const VBE_DISPI_TOTAL_VIDEO_MEMORY_64K: u16 = 128;

const MAX_XRES: u16 = 1920;
const MAX_YRES: u16 = 1200;
const MAX_BPP: u16 = 32;

#[derive(Debug, Clone)]
pub struct BochsVbe {
    index: u16,
    id: u16,
    xres: u16,
    yres: u16,
    bpp: u16,
    enable: u16,
    bank: u16,
    virt_width: u16,
    virt_height: u16,
    x_offset: u16,
    y_offset: u16,
    /// Último modo confirmado al habilitar VBE (valores latched).
    latched_xres: u16,
    latched_yres: u16,
    latched_bpp: u16,
    latched_enable: u16,
    latched_virt_width: u16,
}

impl Default for BochsVbe {
    fn default() -> Self {
        Self::new()
    }
}

impl BochsVbe {
    pub fn new() -> Self {
        Self {
            index: VBE_DISPI_INDEX_ID,
            id: VBE_DISPI_ID0,
            xres: 640,
            yres: 480,
            bpp: 32,
            enable: VBE_DISPI_DISABLED,
            bank: 0,
            virt_width: 0,
            virt_height: 0,
            x_offset: 0,
            y_offset: 0,
            latched_xres: 640,
            latched_yres: 480,
            latched_bpp: 32,
            latched_enable: VBE_DISPI_DISABLED,
            latched_virt_width: 0,
        }
    }

    pub fn handles_port(&self, port: u16) -> bool {
        matches!(port, VBE_DISPI_IOPORT_INDEX | VBE_DISPI_IOPORT_DATA)
    }

    pub fn read_io(&mut self, port: u16, size: u8) -> u32 {
        let val = match port {
            VBE_DISPI_IOPORT_INDEX => self.index,
            VBE_DISPI_IOPORT_DATA => self.read_data(),
            _ => 0xFFFF,
        };
        match size {
            1 => (val & 0xFF) as u32,
            2 => val as u32,
            4 => val as u32, // comportamiento simplificado
            _ => (val & 0xFF) as u32,
        }
    }

    pub fn write_io(&mut self, port: u16, value: u32, size: u8) {
        let v16 = match size {
            1 => (value & 0xFF) as u16,
            2 | 4 => (value & 0xFFFF) as u16,
            _ => (value & 0xFF) as u16,
        };
        match port {
            VBE_DISPI_IOPORT_INDEX => {
                self.index = v16;
            }
            VBE_DISPI_IOPORT_DATA => {
                self.write_data(v16);
            }
            _ => {}
        }
    }

    fn read_data(&self) -> u16 {
        match self.index {
            VBE_DISPI_INDEX_ID => self.id,
            VBE_DISPI_INDEX_XRES => self.xres,
            VBE_DISPI_INDEX_YRES => self.yres,
            VBE_DISPI_INDEX_BPP => self.bpp,
            VBE_DISPI_INDEX_ENABLE => self.enable,
            VBE_DISPI_INDEX_BANK => self.bank,
            VBE_DISPI_INDEX_VIRT_WIDTH => self.virt_width,
            VBE_DISPI_INDEX_VIRT_HEIGHT => self.virt_height,
            VBE_DISPI_INDEX_X_OFFSET => self.x_offset,
            VBE_DISPI_INDEX_Y_OFFSET => self.y_offset,
            VBE_DISPI_INDEX_VIDEO_MEMORY_64K => VBE_DISPI_TOTAL_VIDEO_MEMORY_64K,
            _ => 0,
        }
    }

    fn write_data(&mut self, value: u16) {
        match self.index {
            VBE_DISPI_INDEX_ID => {
                if (VBE_DISPI_ID0..=VBE_DISPI_ID5).contains(&value) {
                    self.id = value;
                }
            }
            VBE_DISPI_INDEX_XRES => {
                self.xres = clamp_res(value, 1, MAX_XRES);
            }
            VBE_DISPI_INDEX_YRES => {
                self.yres = clamp_res(value, 1, MAX_YRES);
            }
            VBE_DISPI_INDEX_BPP => {
                self.bpp = match value {
                    4 | 8 | 15 | 16 | 24 | 32 => value,
                    _ => 32,
                }
                .min(MAX_BPP);
            }
            VBE_DISPI_INDEX_ENABLE => {
                let enabling = (value & VBE_DISPI_ENABLED) != 0
                    && (self.enable & VBE_DISPI_ENABLED) == 0
                    && (value & VBE_DISPI_GETCAPS) == 0;
                self.enable = value & (VBE_DISPI_ENABLED | VBE_DISPI_LFB_ENABLED | VBE_DISPI_NOCLEARMEM | VBE_DISPI_GETCAPS);
                if enabling {
                    self.latched_xres = self.xres;
                    self.latched_yres = self.yres;
                    self.latched_bpp = self.bpp;
                    self.latched_enable = self.enable;
                    self.latched_virt_width = if self.virt_width == 0 { self.xres } else { self.virt_width };
                }
                if (self.enable & VBE_DISPI_ENABLED) == 0 {
                    self.latched_enable = self.enable;
                }
            }
            VBE_DISPI_INDEX_BANK => {
                self.bank = value;
            }
            VBE_DISPI_INDEX_VIRT_WIDTH => {
                self.virt_width = value;
            }
            VBE_DISPI_INDEX_VIRT_HEIGHT => {
                self.virt_height = value;
            }
            VBE_DISPI_INDEX_X_OFFSET => {
                self.x_offset = value;
            }
            VBE_DISPI_INDEX_Y_OFFSET => {
                self.y_offset = value;
            }
            _ => {}
        }
    }

    #[inline]
    pub fn is_enabled(&self) -> bool {
        (self.latched_enable & VBE_DISPI_ENABLED) != 0
    }

    #[inline]
    pub fn is_lfb_enabled(&self) -> bool {
        (self.latched_enable & VBE_DISPI_LFB_ENABLED) != 0
    }

    #[inline]
    pub fn width(&self) -> u32 {
        self.latched_xres as u32
    }

    #[inline]
    pub fn height(&self) -> u32 {
        self.latched_yres as u32
    }

    #[inline]
    pub fn bpp(&self) -> u32 {
        self.latched_bpp as u32
    }

    #[inline]
    pub fn bytes_per_pixel(&self) -> u32 {
        match self.bpp() {
            32 => 4,
            24 => 3,
            15 | 16 => 2,
            8 => 1,
            4 => 1,
            _ => 4,
        }
    }

    #[inline]
    pub fn stride_bytes(&self) -> usize {
        let w = if self.latched_virt_width != 0 { self.latched_virt_width as u32 } else { self.width() };
        (w as usize) * (self.bytes_per_pixel() as usize)
    }

    #[inline]
    pub fn lfb_base(&self) -> u64 {
        VBE_DISPI_LFB_PHYSICAL_ADDRESS
    }

    #[inline]
    pub fn framebuffer_len(&self) -> usize {
        self.stride_bytes().saturating_mul(self.height() as usize)
    }
}

#[inline]
fn clamp_res(v: u16, min: u16, max: u16) -> u16 {
    if v < min { min } else if v > max { max } else { v }
}
