#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RocketState {
    Initializing = 0, // first state, init sensors and logs
    Calibrating = 1,  // manually entered, calibrates baro
    Ready = 2,        // ready for arm/takeoff
    Armed = 3,        // takeoff through apogee, airbrake control active
    Descending = 4,   // airbrake retracted, still logging
    Stopped = 5,       // done
}

impl RocketState {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}