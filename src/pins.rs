//RADIO/LORA
pub const LORA_SCK: u32 = 12;
pub const LORA_MOSI: u32 = 11;
pub const LORA_MISO: u32 = 13;

pub const LORA_CS: u32 = 10;
pub const LORA_RST: u32 = 5;
pub const LORA_BUSY: u32 = 4;
pub const LORA_DIO1: u32 = 1;

pub const LORA_FREQ_HZ: u32 = 915_000_000;

//SPI BUS (shared: IMU + SD)
pub const SPI_SCK: u32 = 36;
pub const SPI_MOSI: u32 = 35;
pub const SPI_MISO: u32 = 37;

pub const IMU_CS: u32 = 34;
pub const IMU_INT: u32 = 33;

pub const SD_CS: u32 = 47;

//I2C BUS 0 (sensors: baro, mag, oled)
pub const I2C0_SDA: u32 = 17;
pub const I2C0_SCL: u32 = 18;

//I2C BUS 1 (pmu, rtc)
pub const I2C1_SDA: u32 = 42;
pub const I2C1_SCL: u32 = 41;

pub const PMU_IRQ: u32 = 40;

//GNSS
pub const GNSS_TX: u32 = 8;
pub const GNSS_RX: u32 = 9;
pub const GNSS_PPS: u32 = 6;