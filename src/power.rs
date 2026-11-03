//! Power rail driver for the TBeam S3.
//!
//! Gates the AXP2101 PMU's ALDO rails that feed LoRa, GPS, and the OLED
//! screen.
//!
//! Rail assignment (ALDO1 = general 3V3 peripherals, ALDO2 = LoRa,
//! ALDO3 = GPS, ALDO4 = OLED) follows LilyGo's common T-Beam Supreme
//! mapping. It is not documented anywhere in this repo and has not been
//! measured against real hardware -- verify it against your schematic
//! before relying on it.

use embedded_hal_async::i2c::I2c as AsyncI2c;

use crate::pins;

const REG_LDO_ENABLE1: u8 = 0x91; // ALDO1-4 / BLDO1-2 / DLDO1-2 / CPUSLDO enable bits

const REG_ALDO2_VOLTAGE: u8 = 0x98;
const REG_ALDO3_VOLTAGE: u8 = 0x99;
const REG_ALDO4_VOLTAGE: u8 = 0x9A;

// ADC channel enable + data registers. The AXP2101 has no current/power
// measurement register at all -- only these voltage ADCs and a fuel-gauge
// percentage. Confirmed against LilyGo's XPowersLib (AXP2101Constants.h /
// XPowersAXP2101.hpp): zero CURRENT/IBAT-style registers exist on this chip.
const REG_ADC_CHANNEL_CTRL: u8 = 0x30;
const ADC_CHANNEL_VBAT_EN: u8 = 1 << 0;
const ADC_CHANNEL_VBUS_EN: u8 = 1 << 2;

const REG_ADC_VBAT_H: u8 = 0x34; // 13-bit result, 1 LSB = 1 mV
const REG_ADC_VBAT_L: u8 = 0x35;
const REG_ADC_VBUS_H: u8 = 0x38; // 14-bit result, 1 LSB = 1 mV
const REG_ADC_VBUS_L: u8 = 0x39;
const REG_BAT_PERCENT: u8 = 0xA4; // 0-100, or >100 when no battery/gauge not ready

/// A switchable ALDO rail on the AXP2101.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rail {
    Aldo2,
    Aldo3,
    Aldo4,
}

impl Rail {
    const fn enable_bit(self) -> u8 {
        match self {
            Rail::Aldo2 => 1 << 1,
            Rail::Aldo3 => 1 << 2,
            Rail::Aldo4 => 1 << 3,
        }
    }

    const fn voltage_register(self) -> u8 {
        match self {
            Rail::Aldo2 => REG_ALDO2_VOLTAGE,
            Rail::Aldo3 => REG_ALDO3_VOLTAGE,
            Rail::Aldo4 => REG_ALDO4_VOLTAGE,
        }
    }
}

/// Error from a PMU operation: either the underlying I2C bus failed, or a
/// requested voltage was outside the ALDO range (500-3500mV).
#[derive(Debug)]
pub enum PmuError<E> {
    I2c(E),
    VoltageOutOfRange,
}

impl<E> From<E> for PmuError<E> {
    fn from(e: E) -> Self {
        PmuError::I2c(e)
    }
}

struct Pmu<I2C> {
    i2c: I2C,
}

impl<I2C, E> Pmu<I2C>
where
    I2C: AsyncI2c<Error = E>,
{
    fn new(i2c: I2C) -> Self {
        Self { i2c }
    }

    async fn read_reg(&mut self, reg: u8) -> Result<u8, E> {
        let mut buf = [0u8; 1];
        self.i2c
            .write_read(pins::PMU_I2C_ADDRESS, &[reg], &mut buf)
            .await?;
        Ok(buf[0])
    }

    async fn write_reg(&mut self, reg: u8, value: u8) -> Result<(), E> {
        self.i2c.write(pins::PMU_I2C_ADDRESS, &[reg, value]).await
    }

    async fn set_enabled(&mut self, rail: Rail, enabled: bool) -> Result<(), E> {
        let bit = rail.enable_bit();
        let current = self.read_reg(REG_LDO_ENABLE1).await?;
        let next = if enabled { current | bit } else { current & !bit };
        self.write_reg(REG_LDO_ENABLE1, next).await
    }

    async fn set_voltage_mv(&mut self, rail: Rail, millivolts: u16) -> Result<(), PmuError<E>> {
        if !(500..=3500).contains(&millivolts) {
            return Err(PmuError::VoltageOutOfRange);
        }
        let code = ((millivolts - 500) / 100) as u8;
        self.write_reg(rail.voltage_register(), code).await?;
        Ok(())
    }

    async fn power_on(&mut self, rail: Rail, millivolts: u16) -> Result<(), PmuError<E>> {
        self.set_voltage_mv(rail, millivolts).await?;
        self.set_enabled(rail, true).await?;
        Ok(())
    }

    async fn power_off(&mut self, rail: Rail) -> Result<(), E> {
        self.set_enabled(rail, false).await
    }

    /// High/low register pair -> raw ADC value, masking off the unused
    /// high bits per the chip's per-channel result width.
    async fn read_adc_pair(&mut self, reg_h: u8, reg_l: u8, high_mask: u8) -> Result<u16, E> {
        let h = self.read_reg(reg_h).await?;
        let l = self.read_reg(reg_l).await?;
        Ok(((h & high_mask) as u16) << 8 | l as u16)
    }

    async fn enable_telemetry_adc(&mut self) -> Result<(), E> {
        let current = self.read_reg(REG_ADC_CHANNEL_CTRL).await?;
        self.write_reg(
            REG_ADC_CHANNEL_CTRL,
            current | ADC_CHANNEL_VBAT_EN | ADC_CHANNEL_VBUS_EN,
        )
        .await
    }

    async fn battery_voltage_mv(&mut self) -> Result<u16, E> {
        self.read_adc_pair(REG_ADC_VBAT_H, REG_ADC_VBAT_L, 0x1F).await
    }

    async fn vbus_voltage_mv(&mut self) -> Result<u16, E> {
        self.read_adc_pair(REG_ADC_VBUS_H, REG_ADC_VBUS_L, 0x3F).await
    }

    /// `None` if the fuel gauge hasn't produced a valid reading yet (raw
    /// value > 100).
    async fn battery_percent(&mut self) -> Result<Option<u8>, E> {
        let raw = self.read_reg(REG_BAT_PERCENT).await?;
        Ok((raw <= 100).then_some(raw))
    }
}

/// A snapshot of PMU telemetry. The AXP2101 exposes voltages and a
/// fuel-gauge percentage only -- it has no current/power measurement, so
/// there is no `*_ma`/`*_mw` field here. See the ADC register comment above.
#[derive(Clone, Copy, Debug, Default)]
pub struct Telemetry {
    pub battery_mv: u16,
    pub vbus_mv: u16,
    pub battery_percent: Option<u8>,
}

/// The TBeam S3's power rails for LoRa, GPS, and the OLED screen (via the
/// AXP2101 PMU).
pub struct Power<I2C> {
    pmu: Pmu<I2C>,
}

impl<I2C, E> Power<I2C>
where
    I2C: AsyncI2c<Error = E>,
{
    /// `i2c` must already be addressing the PMU bus
    /// ([`pins::PMU_SDA`]/[`pins::PMU_SCL`]).
    pub fn new(i2c: I2C) -> Self {
        Self {
            pmu: Pmu::new(i2c),
        }
    }

    /// Powers up LoRa, GPS, and the OLED at 3.3V.
    pub async fn power_on_all(&mut self) -> Result<(), PmuError<E>> {
        self.lora_on().await?;
        self.gps_on().await?;
        self.oled_on().await?;
        Ok(())
    }

    /// Powers down LoRa, GPS, and the OLED.
    pub async fn power_off_all(&mut self) -> Result<(), E> {
        self.pmu.power_off(Rail::Aldo2).await?;
        self.pmu.power_off(Rail::Aldo3).await?;
        self.pmu.power_off(Rail::Aldo4).await?;
        Ok(())
    }

    pub async fn lora_on(&mut self) -> Result<(), PmuError<E>> {
        self.pmu.power_on(Rail::Aldo2, 3300).await
    }

    pub async fn lora_off(&mut self) -> Result<(), E> {
        self.pmu.power_off(Rail::Aldo2).await
    }

    pub async fn gps_on(&mut self) -> Result<(), PmuError<E>> {
        self.pmu.power_on(Rail::Aldo3, 3300).await
    }

    pub async fn gps_off(&mut self) -> Result<(), E> {
        self.pmu.power_off(Rail::Aldo3).await
    }

    pub async fn oled_on(&mut self) -> Result<(), PmuError<E>> {
        self.pmu.power_on(Rail::Aldo4, 3300).await
    }

    pub async fn oled_off(&mut self) -> Result<(), E> {
        self.pmu.power_off(Rail::Aldo4).await
    }

    /// Turns on the battery/VBUS voltage ADC channels. Call once before the
    /// first [`Power::read_telemetry`].
    pub async fn enable_telemetry(&mut self) -> Result<(), E> {
        self.pmu.enable_telemetry_adc().await
    }

    /// Reads current battery/VBUS voltage and battery percentage.
    /// [`Power::enable_telemetry`] must have been called first.
    pub async fn read_telemetry(&mut self) -> Result<Telemetry, E> {
        Ok(Telemetry {
            battery_mv: self.pmu.battery_voltage_mv().await?,
            vbus_mv: self.pmu.vbus_voltage_mv().await?,
            battery_percent: self.pmu.battery_percent().await?,
        })
    }
}
