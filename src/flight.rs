use crate::sensor::Sensors;
use crate::state::RocketState;
use crate::storage::Logger;

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
        // get new sensors and assemble new logframe
    }

    pub fn update_state(&mut self, new_state: RocketState) {
        self.state = new_state;
    }
}