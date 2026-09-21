use crate::state::RocketState;
use bytemuck::{Pod, Zeroable};

#[repr(C, packed)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct LogFrame {
    pub esp_seconds: u64,
    pub time: u64,
    pub velocity: u32,
    pub accel_x: f32,
    pub accel_y: f32,
    pub accel_z: f32,
    pub altitude: f32,
    pub brake_position: i32,
    pub degrees: i16,
    pub state: u8, // RocketState::as_u8()
}

impl LogFrame {
    pub fn new(
        esp_seconds: u64,
        time: u64,
        velocity: f32,
        accel_x: f32,
        accel_y: f32,
        accel_z: f32,
        altitude: f32,
        brake_position: i32,
        degrees: i16,
        state: RocketState,
    ) -> Self {
        LogFrame {
            esp_seconds,
            time,
            velocity: velocity.to_bits(),
            accel_x,
            accel_y,
            accel_z,
            altitude,
            brake_position,
            degrees,
            state: state.as_u8(),
        }
    }

    pub fn bytes(&self) -> &[u8] {
        bytemuck::bytes_of(self)
    }
}