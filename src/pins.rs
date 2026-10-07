//! Pin assignments for the TBeam S3 (LilyGo T-Beam Supreme).
//!
//! Verified against LilyGo's reference `utilities.h` for
//! `T_BEAM_S3_SUPREME_SX1262` (LilyGo-LoRa-Series repo, examples/PMU).

/// PMU/RTC I2C bus (AXP2101 power management IC).
pub const PMU_SDA: u8 = 42;
pub const PMU_SCL: u8 = 41;
pub const PMU_IRQ: u8 = 40;

/// AXP2101 I2C address (fixed by the chip).
pub const PMU_I2C_ADDRESS: u8 = 0x34;

/// Main peripheral I2C bus (OLED, sensors). Separate from the PMU bus.
pub const OLED_SDA: u8 = 17;
pub const OLED_SCL: u8 = 18;

/// SH1106 I2C address. T-Beam Supreme sub-variants (VHF/UHF) wire the
/// display to either 0x3D or 0x3C, with a QMC6310N magnetometer on the
/// other address -- probe both at init rather than assuming one.
pub const OLED_I2C_ADDRESS_PRIMARY: u8 = 0x3D;
pub const OLED_I2C_ADDRESS_FALLBACK: u8 = 0x3C;

/// LoRa radio BUSY signal (SX1262). Not a status LED -- the Supreme has no
/// board-level status LED exposed in LilyGo's reference pin map. If you wire
/// up your own bring-up LED, pick a free GPIO and avoid this one.
pub const LORA_BUSY: u8 = 4;
