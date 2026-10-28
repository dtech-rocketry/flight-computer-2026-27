//! Flight state machine: idle -> armed -> burn -> coast -> descent -> landed.

#[derive(Debug, Default, Clone, Copy)]
pub struct SensorSample {
    /// vertical acceleration, in g, with +1g baseline removed (so ~0 at rest).
    pub accel_g: f32,
    /// altitude above the pad, in meters.
    pub altitude_m: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlightState {
    Idle,
    Armed,
    Burn,
    Coast,
    Descent,
    Landed,
}

impl FlightState {
    pub const fn label(self) -> &'static str {
        match self {
            FlightState::Idle => "IDLE",
            FlightState::Armed => "ARMED",
            FlightState::Burn => "BURN",
            FlightState::Coast => "COAST",
            FlightState::Descent => "DESCENT",
            FlightState::Landed => "LANDED",
        }
    }

    /// encodes as a `u8` so the state can live in an [`core::sync::atomic::AtomicU8`]
    /// shared between the flight loop and display tasks.
    pub const fn to_u8(self) -> u8 {
        match self {
            FlightState::Idle => 0,
            FlightState::Armed => 1,
            FlightState::Burn => 2,
            FlightState::Coast => 3,
            FlightState::Descent => 4,
            FlightState::Landed => 5,
        }
    }

    /// inverse of [`FlightState::to_u8`]. Any value outside `0..=5` (should
    /// not happen since it's only ever written via `to_u8`) falls back to
    /// `Idle`.
    pub const fn from_u8(value: u8) -> Self {
        match value {
            1 => FlightState::Armed,
            2 => FlightState::Burn,
            3 => FlightState::Coast,
            4 => FlightState::Descent,
            5 => FlightState::Landed,
            _ => FlightState::Idle,
        }
    }
}

const DEBOUNCE_SAMPLES: u8 = 5;

const BURN_START_ACCEL_G: f32 = 2.0;
const BURN_END_ACCEL_G: f32 = 0.5;
const LANDED_ACCEL_G: f32 = 0.2;

pub struct FlightComputer {
    state: FlightState,
    /// Highest altitude seen this flight, used to detect apogee.
    peak_altitude_m: f32,
    /// How many consecutive samples have met the current candidate
    /// transition's threshold.
    debounce_count: u8,
}

impl Default for FlightComputer {
    fn default() -> Self {
        Self {
            state: FlightState::Idle,
            peak_altitude_m: 0.0,
            debounce_count: 0,
        }
    }
}

impl FlightComputer {
    pub fn state(&self) -> FlightState {
        self.state
    }

    /// Feeds one sensor sample in and returns the (possibly unchanged)
    /// state after applying it.
    pub fn update(&mut self, sample: SensorSample) -> FlightState {
        if sample.altitude_m > self.peak_altitude_m {
            self.peak_altitude_m = sample.altitude_m;
        }

        let candidate = next_state(self.state, sample, self.peak_altitude_m);

        if candidate == self.state {
            self.debounce_count = 0;
            return self.state;
        }

        self.debounce_count += 1;
        if self.debounce_count >= DEBOUNCE_SAMPLES {
            self.state = candidate;
            self.debounce_count = 0;
        }

        self.state
    }
}


fn next_state(current: FlightState, sample: SensorSample, peak_altitude_m: f32) -> FlightState {
    match current {
        FlightState::Idle => FlightState::Idle,
        FlightState::Armed => {
            if sample.accel_g > BURN_START_ACCEL_G {
                FlightState::Burn
            } else {
                FlightState::Armed
            }
        }
        FlightState::Burn => {
            if sample.accel_g < BURN_END_ACCEL_G {
                FlightState::Coast
            } else {
                FlightState::Burn
            }
        }
        FlightState::Coast => {
            // Apogee: altitude has fallen off noticeably from its peak.
            if sample.altitude_m < peak_altitude_m - 1.0 {
                FlightState::Descent
            } else {
                FlightState::Coast
            }
        }
        FlightState::Descent => {
            if sample.accel_g.abs() < LANDED_ACCEL_G && sample.altitude_m < 2.0 {
                FlightState::Landed
            } else {
                FlightState::Descent
            }
        }
        FlightState::Landed => FlightState::Landed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(accel_g: f32, altitude_m: f32) -> SensorSample {
        SensorSample { accel_g, altitude_m }
    }

    #[test]
    fn burn_requires_sustained_accel() {
        let mut fc = FlightComputer {
            state: FlightState::Armed,
            ..Default::default()
        };
        for _ in 0..DEBOUNCE_SAMPLES - 1 {
            assert_eq!(fc.update(sample(3.0, 0.0)), FlightState::Armed);
        }
        assert_eq!(fc.update(sample(3.0, 0.0)), FlightState::Burn);
    }

    #[test]
    fn single_noisy_sample_does_not_flip_state() {
        let mut fc = FlightComputer {
            state: FlightState::Armed,
            ..Default::default()
        };
        fc.update(sample(3.0, 0.0));
        assert_eq!(fc.update(sample(0.0, 0.0)), FlightState::Armed);
    }
}
