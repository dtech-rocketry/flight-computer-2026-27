//! LoRa telemetry broadcast over the T-Beam Supreme's onboard SX1262.
//!
//! Point-to-point LoRa, not LoRaWAN or Meshtastic -- the receiver is
//! expected to be a second device (e.g. a Seeed XIAO ESP32S3 + Wio-SX1262
//! kit) running its own matching firmware, listening with the same
//! frequency/spreading-factor/bandwidth/coding-rate and decoding the same
//! [`TelemetryPacket`] wire format. There is no over-the-air encryption or
//! addressing here; anything in range on this frequency can receive it.
//!
//! SPI pins and radio config (DIO2-as-RF-switch, DIO3 TCXO voltage, DC-DC
//! regulator) are verified against Meshtastic's
//! `variants/esp32s3/tbeam-s3-core/variant.h`, which targets this same
//! board.

use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::Async;
use esp_hal::gpio::{Input, Output};
use esp_hal::spi::master::Spi;
use log::{info, warn};
use lora_phy::LoRa;
use lora_phy::iv::GenericSx126xInterfaceVariant;
use lora_phy::mod_params::{Bandwidth, CodingRate, ModulationParams, RadioError, SpreadingFactor};
use lora_phy::sx126x::{self, Sx1262, Sx126x, TcxoCtrlVoltage};

use crate::flight::FlightState;
use crate::power::Telemetry;

/// Shared channel for both ends of the link. Must match whatever the
/// receiver firmware is configured for -- 915 MHz is the US ISM band;
/// pick the right one for your region.
pub const FREQUENCY_IN_HZ: u32 = 915_000_000;

/// 500 kHz + SF7 puts this link in FCC 15.247's Digital Transformation
/// System (DTS) bucket rather than the hybrid bucket, so no frequency
/// hopping is required (avoids having to implement FHSS across 50+
/// channels). At these settings our 6-byte payload's time-on-air is
/// ~11 ms (8-symbol preamble + ~32 payload symbols, each 256us at
/// SF7/500kHz) -- far under the 400ms/20s dwell-time limit that would
/// otherwise force hopping. Don't drop the bandwidth below 500kHz or
/// raise the spreading factor without re-deriving this margin.
const SPREADING_FACTOR: SpreadingFactor = SpreadingFactor::_7;
const BANDWIDTH: Bandwidth = Bandwidth::_500KHz;
const CODING_RATE: CodingRate = CodingRate::_4_8;
const PREAMBLE_LENGTH: u16 = 8;
const TX_POWER_DBM: i32 = 20;

/// Gates whether telemetry gets broadcast at all. Defaults to on now that
/// a real radio is wired up; flip with [`set_broadcast_enabled`] to keep
/// the radio quiet (e.g. for bench testing without a receiver in range).
static BROADCAST_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_broadcast_enabled(enabled: bool) {
    BROADCAST_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn broadcast_enabled() -> bool {
    BROADCAST_ENABLED.load(Ordering::Relaxed)
}

/// Wire-format telemetry packet: flight state + power telemetry, packed
/// into a fixed 6-byte layout so it's cheap to send over LoRa's small
/// payload budget.
#[derive(Debug, Clone, Copy)]
pub struct TelemetryPacket {
    pub flight_state: FlightState,
    pub battery_mv: u16,
    pub vbus_mv: u16,
    /// `None` encodes as `0xFF` on the wire (see [`Telemetry::battery_percent`]).
    pub battery_percent: Option<u8>,
}

impl TelemetryPacket {
    pub fn new(flight_state: FlightState, telemetry: &Telemetry) -> Self {
        Self {
            flight_state,
            battery_mv: telemetry.battery_mv,
            vbus_mv: telemetry.vbus_mv,
            battery_percent: telemetry.battery_percent,
        }
    }

    pub fn to_bytes(&self) -> [u8; 6] {
        let [bat_hi, bat_lo] = self.battery_mv.to_be_bytes();
        let [vbus_hi, vbus_lo] = self.vbus_mv.to_be_bytes();
        [
            self.flight_state.to_u8(),
            bat_hi,
            bat_lo,
            vbus_hi,
            vbus_lo,
            self.battery_percent.unwrap_or(0xFF),
        ]
    }
}

type RadioSpiDevice = ExclusiveDevice<Spi<'static, Async>, Output<'static>, Delay>;
type RadioKind = Sx126x<
    RadioSpiDevice,
    GenericSx126xInterfaceVariant<Output<'static>, Input<'static>>,
    Sx1262,
>;

/// Summary of the radio link for display purposes (e.g. on the OLED).
/// Distinct from [`broadcast_enabled`]/[`set_broadcast_enabled`], which only
/// gate whether packets go out at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadioStatus {
    /// Broadcast is disabled via [`set_broadcast_enabled`].
    Disabled,
    /// Broadcast is enabled but no send has been attempted yet.
    Idle,
    /// Most recent send succeeded.
    TxOk,
    /// Most recent send failed.
    TxError,
}

impl RadioStatus {
    pub const fn label(self) -> &'static str {
        match self {
            RadioStatus::Disabled => "OFF",
            RadioStatus::Idle => "IDLE",
            RadioStatus::TxOk => "TX OK",
            RadioStatus::TxError => "TX ERR",
        }
    }
}

/// Owns the SX1262 and the modulation parameters negotiated with it at
/// init time, so every send reuses the same radio config.
pub struct Radio {
    lora: LoRa<RadioKind, Delay>,
    modulation_params: ModulationParams,
    /// Result of the most recent send attempt, if any.
    last_tx_ok: Option<bool>,
}

impl Radio {
    /// Takes ownership of the SPI peripheral and the four SX1262 control
    /// lines (NSS, RESET, DIO1, BUSY), initializes the radio, and fixes
    /// the modulation parameters for the lifetime of this `Radio`.
    pub async fn new(
        spi: Spi<'static, Async>,
        nss: Output<'static>,
        reset: Output<'static>,
        dio1: Input<'static>,
        busy: Input<'static>,
    ) -> Result<Self, RadioError> {
        let spi_device = ExclusiveDevice::new(spi, nss, Delay)
            .expect("failed to configure SX1262 SPI device (CS pin is infallible)");

        // No separate rf_switch_rx/tx pins: the SX1262 drives its own
        // antenna switch from DIO2 (SX126X_DIO2_AS_RF_SWITCH).
        let iv = GenericSx126xInterfaceVariant::new(reset, dio1, busy, None, None)
            .expect("failed to configure SX1262 interface variant");

        let config = sx126x::Config {
            chip: Sx1262,
            tcxo_ctrl: Some(TcxoCtrlVoltage::Ctrl1V8),
            use_dcdc: true,
            rx_boost: true,
        };

        let mut lora = LoRa::new(Sx126x::new(spi_device, iv, config), false, Delay).await?;

        let modulation_params = lora.create_modulation_params(
            SPREADING_FACTOR,
            BANDWIDTH,
            CODING_RATE,
            FREQUENCY_IN_HZ,
        )?;

        Ok(Self {
            lora,
            modulation_params,
            last_tx_ok: None,
        })
    }

    /// Current link status for display purposes; see [`RadioStatus`].
    pub fn status(&self) -> RadioStatus {
        if !broadcast_enabled() {
            return RadioStatus::Disabled;
        }
        match self.last_tx_ok {
            None => RadioStatus::Idle,
            Some(true) => RadioStatus::TxOk,
            Some(false) => RadioStatus::TxError,
        }
    }

    /// Builds a [`TelemetryPacket`] from the current state and sends it,
    /// unless broadcast is disabled via [`set_broadcast_enabled`]. Logs
    /// and returns on the first radio error rather than panicking --
    /// a dropped telemetry packet shouldn't take down the flight computer.
    /// Records the outcome in `last_tx_ok` so [`Radio::status`] reflects it.
    pub async fn broadcast(&mut self, flight_state: FlightState, telemetry: &Telemetry) {
        if !broadcast_enabled() {
            return;
        }

        let packet = TelemetryPacket::new(flight_state, telemetry);
        let bytes = packet.to_bytes();

        let mut tx_packet_params = match self.lora.create_tx_packet_params(
            PREAMBLE_LENGTH,
            false,
            true,
            false,
            &self.modulation_params,
        ) {
            Ok(params) => params,
            Err(e) => {
                warn!("lora: failed to build tx packet params: {:?}", e);
                self.last_tx_ok = Some(false);
                return;
            }
        };

        if let Err(e) = self
            .lora
            .prepare_for_tx(
                &self.modulation_params,
                &mut tx_packet_params,
                TX_POWER_DBM,
                &bytes,
            )
            .await
        {
            warn!("lora: prepare_for_tx failed: {:?}", e);
            self.last_tx_ok = Some(false);
            return;
        }

        if let Err(e) = self.lora.tx().await {
            warn!("lora: tx failed: {:?}", e);
            self.last_tx_ok = Some(false);
            return;
        }

        info!("lora tx: {:02x?}", bytes);
        self.last_tx_ok = Some(true);

        if let Err(e) = self.lora.sleep(false).await {
            warn!("lora: sleep failed: {:?}", e);
        }
    }
}
