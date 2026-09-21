use std::cell::RefCell;
use std::time::Instant;
use embedded_hal_bus::i2c::RefCellDevice;
use esp_idf_hal::i2c::I2cDriver;
use esp_idf_hal::delay::Delay;
use qmi8658::Qmi8658;
use qmi8658::command::register::ctrl2::{Ctrl2Register, AccelerometerFS, AccelerometerODR};
use qmi8658::command::register::ctrl3::{Ctrl3Register, GyroscopeFS, GyroscopeODR};

pub struct Sensors {
    barometer: bme280::i2c::BME280<RefCellDevice<'static, I2cDriver<'static>>>,
    imu: Qmi8658<RefCellDevice<'static, I2cDriver<'static>>, Delay>,
    delay: Delay,
    ground_pressure_hpa: f32,
    accel_bias_z: f32,
    velocity_mps: f32,
    last_poll: Instant,
}

pub struct Reading {
    pub altitude_m: f32,
    pub velocity_mps: f32,
    pub accel_x: f32,
    pub accel_y: f32,
    pub accel_z: f32,
}

impl Sensors {
    pub fn new(i2c_driver: I2cDriver<'static>) -> anyhow::Result<Self> {
        let bus: &'static RefCell<I2cDriver<'static>> = Box::leak(Box::new(RefCell::new(i2c_driver)));
        let mut delay = Delay::new_default();

        let bme_dev = RefCellDevice::new(bus);
        let mut bme = bme280::i2c::BME280::new_primary(bme_dev);
        bme.init(&mut delay).map_err(|e| anyhow::anyhow!("{:?}", e))?;

        let imu_dev = RefCellDevice::new(bus);
        let mut imu = Qmi8658::new(imu_dev, Delay::new_default());

        let mut ctrl2 = Ctrl2Register(0);
        ctrl2.set_afs(AccelerometerFS::FS4G);
        ctrl2.set_aodr(AccelerometerODR::NormalAODR3);
        imu.set_ctrl2(ctrl2).map_err(|e| anyhow::anyhow!("{:?}", e))?;

        let mut ctrl3 = Ctrl3Register(0);
        ctrl3.set_gfs(GyroscopeFS::DPS256);
        ctrl3.set_godr(GyroscopeODR::NormalGORD3);
        imu.set_ctrl3(ctrl3).map_err(|e| anyhow::anyhow!("{:?}", e))?;

        imu.set_sensors_enable(true, true).map_err(|e| anyhow::anyhow!("{:?}", e))?;

        let mut sensors = Self {
            barometer: bme,
            imu,
            delay,
            ground_pressure_hpa: 1013.25,
            accel_bias_z: 0.0,
            velocity_mps: 0.0,
            last_poll: Instant::now(),
        };
        sensors.calibrate_ground()?;

        Ok(sensors)
    }

    pub fn calibrate_ground(&mut self) -> anyhow::Result<()> {
        let m = self.barometer.measure(&mut self.delay).map_err(|e| anyhow::anyhow!("{:?}", e))?;
        self.ground_pressure_hpa = m.pressure / 100.0;

        let accel = self.imu.get_acceleration().map_err(|e| anyhow::anyhow!("{:?}", e))?;
        self.accel_bias_z = accel.z;

        self.velocity_mps = 0.0;
        self.last_poll = Instant::now();

        Ok(())
    }

    pub fn poll(&mut self) -> anyhow::Result<Reading> {
        let env = self.barometer.measure(&mut self.delay).map_err(|e| anyhow::anyhow!("{:?}", e))?;
        let accel = self.imu.get_acceleration().map_err(|e| anyhow::anyhow!("{:?}", e))?;

        let pressure_hpa = env.pressure / 100.0;
        let altitude_m = 44330.0 * (1.0 - (pressure_hpa / self.ground_pressure_hpa).powf(0.1903));

        let now = Instant::now();
        let dt = now.duration_since(self.last_poll).as_secs_f32();
        self.last_poll = now;

        let vertical_accel_ms2 = (accel.z - self.accel_bias_z) * 9.80665;
        self.velocity_mps += vertical_accel_ms2 * dt;

        Ok(Reading {
            altitude_m,
            velocity_mps: self.velocity_mps,
            accel_x: accel.x,
            accel_y: accel.y,
            accel_z: accel.z,
        })
    }
}