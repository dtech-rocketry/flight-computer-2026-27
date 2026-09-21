use crate::frame::LogFrame;
use crate::sensor::Sensors;
use crate::state::RocketState;
use crate::storage::Logger;
use esp_idf_svc::systime::EspSystemTime;
use std::time::SystemTime;

pub struct FlightComputer {
    logger: Logger,
    sensors: Sensors,
    state: RocketState,
}

impl FlightComputer {
    pub fn new(logger: Logger, sensors: Sensors, state: RocketState) -> Self {
        Self {
            logger,
            sensors,
            state,
        }
    }

    pub fn tick(&mut self) {
        let result = match self.sensors.poll() {
            Ok(r) => r,
            Err(_) => return,
        };

        let boot_ms = EspSystemTime {}.now().as_millis() as u64;
        let unix_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let airbrake_position = 0;
        let event_flags = 0;

        let frame = LogFrame::new(
            boot_ms,
            unix_ms,
            result.velocity_mps,
            result.accel_x,
            result.accel_y,
            result.accel_z,
            result.altitude_m,
            airbrake_position,
            event_flags,
            self.state,
        );
        self.logger.push(frame);
        // TODO: airbrake logic here
    }

    pub fn update_state(&mut self, new_state: RocketState) {
        self.state = new_state;
    }
}