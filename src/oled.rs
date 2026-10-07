//! Minimal SH1106 128x64 I2C display driver and renderer.
//!
//! Hand-rolled rather than using the `sh1106` crate from crates.io: that
//! crate only implements `embedded-hal` 0.2 (blocking) traits, and esp-hal
//! 1.2.2's I2C driver implements `embedded-hal`/`embedded-hal-async` 1.0
//! only -- the two aren't compatible without an adapter shim. Command bytes
//! below are cross-checked against the `sh1106` crate's `command.rs` /
//! `properties.rs` (rust-embedded-community/sh1106) for correctness.

use core::fmt::Write as _;

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::text::{Baseline, Text};
use embedded_hal_async::i2c::I2c as AsyncI2c;
use heapless::String;

use crate::pins;
use crate::power::Telemetry;

pub const WIDTH: u32 = 128;
pub const HEIGHT: u32 = 64;
const PAGES: usize = (HEIGHT / 8) as usize;
const BUFFER_LEN: usize = WIDTH as usize * PAGES;

/// SH1106 driver RAM is 132 columns wide; most modules center the 128px
/// panel with a 2-column offset. Verify against your actual hardware if
/// the image appears shifted left/right.
const COLUMN_OFFSET: u8 = 2;

const CMD_CONTROL_BYTE: u8 = 0x00;
const DATA_CONTROL_BYTE: u8 = 0x40;

const TEXT_STYLE: MonoTextStyle<'static, BinaryColor> =
    MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

pub struct Oled<I2C> {
    i2c: I2C,
    address: u8,
    buffer: [u8; BUFFER_LEN],
}

impl<I2C, E> Oled<I2C>
where
    I2C: AsyncI2c<Error = E>,
{
    /// Probes [`pins::OLED_I2C_ADDRESS_PRIMARY`] then
    /// [`pins::OLED_I2C_ADDRESS_FALLBACK`] (see the doc comment on those
    /// constants), initializes the controller, and clears the screen.
    pub async fn new(mut i2c: I2C) -> Result<Self, E> {
        let address = Self::probe_address(&mut i2c).await?;
        let mut oled = Self {
            i2c,
            address,
            buffer: [0; BUFFER_LEN],
        };
        oled.init().await?;
        oled.flush().await?;
        Ok(oled)
    }

    async fn probe_address(i2c: &mut I2C) -> Result<u8, E> {
        // A no-op command write is enough to check for an ACK.
        if i2c
            .write(pins::OLED_I2C_ADDRESS_PRIMARY, &[CMD_CONTROL_BYTE, 0xE3])
            .await
            .is_ok()
        {
            return Ok(pins::OLED_I2C_ADDRESS_PRIMARY);
        }
        i2c.write(pins::OLED_I2C_ADDRESS_FALLBACK, &[CMD_CONTROL_BYTE, 0xE3])
            .await?;
        Ok(pins::OLED_I2C_ADDRESS_FALLBACK)
    }

    async fn send_commands(&mut self, cmds: &[u8]) -> Result<(), E> {
        let mut buf = heapless::Vec::<u8, 8>::new();
        buf.push(CMD_CONTROL_BYTE).ok();
        buf.extend_from_slice(cmds).ok();
        self.i2c.write(self.address, &buf).await
    }

    async fn init(&mut self) -> Result<(), E> {
        self.send_commands(&[0xAE]).await?; // display off
        self.send_commands(&[0xD5, 0x80]).await?; // clock divide
        self.send_commands(&[0xA8, 0x3F]).await?; // multiplex ratio: 63 (64 rows)
        self.send_commands(&[0xD3, 0x00]).await?; // display offset: 0
        self.send_commands(&[0x40]).await?; // start line: 0
        self.send_commands(&[0xAD, 0x8B]).await?; // charge pump on
        self.send_commands(&[0xA1]).await?; // segment remap
        self.send_commands(&[0xC8]).await?; // COM scan direction, reversed
        self.send_commands(&[0xDA, 0x12]).await?; // COM pins config
        self.send_commands(&[0x81, 0x80]).await?; // contrast
        self.send_commands(&[0xD9, 0xF1]).await?; // precharge period
        self.send_commands(&[0xDB, 0x40]).await?; // VCOMH deselect: auto
        self.send_commands(&[0xA4]).await?; // resume to RAM content display
        self.send_commands(&[0xA6]).await?; // normal (not inverted)
        self.send_commands(&[0xAF]).await?; // display on
        Ok(())
    }

    pub fn clear(&mut self) {
        self.buffer.fill(0);
    }

    /// Pushes the framebuffer to the display, one page (8 rows) at a time.
    pub async fn flush(&mut self) -> Result<(), E> {
        for page in 0..PAGES {
            self.send_commands(&[
                0xB0 | page as u8,           // set page address
                COLUMN_OFFSET & 0x0F,        // set column address, low nibble
                0x10 | (COLUMN_OFFSET >> 4), // set column address, high nibble
            ])
            .await?;

            let start = page * WIDTH as usize;
            let row = &self.buffer[start..start + WIDTH as usize];

            // Keep each I2C transaction's stack buffer small.
            for chunk in row.chunks(32) {
                let mut buf = heapless::Vec::<u8, 33>::new();
                buf.push(DATA_CONTROL_BYTE).ok();
                buf.extend_from_slice(chunk).ok();
                self.i2c.write(self.address, &buf).await?;
            }
        }
        Ok(())
    }

    /// Draws a power-telemetry summary plus the most recent log lines
    /// (oldest first, newest last) and pushes the frame to the display.
    pub async fn render(
        &mut self,
        telemetry: &Telemetry,
    ) -> Result<(), E> {
        self.clear();

        let mut line: String<32> = String::new();

        line.clear();
        let _ = match telemetry.battery_percent {
            Some(pct) => write!(line, "BAT {}mV {}%", telemetry.battery_mv, pct),
            None => write!(line, "BAT {}mV --%", telemetry.battery_mv),
        };
        let _ = Text::with_baseline(&line, Point::new(0, 0), TEXT_STYLE, Baseline::Top)
            .draw(self);

        line.clear();
        let _ = write!(line, "VBUS {}mV", telemetry.vbus_mv);
        let _ = Text::with_baseline(&line, Point::new(0, 11), TEXT_STYLE, Baseline::Top)
            .draw(self);

        self.flush().await
    }
}

impl<I2C> OriginDimensions for Oled<I2C> {
    fn size(&self) -> Size {
        Size::new(WIDTH, HEIGHT)
    }
}

impl<I2C> DrawTarget for Oled<I2C> {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels {
            if coord.x < 0 || coord.y < 0 || coord.x as u32 >= WIDTH || coord.y as u32 >= HEIGHT {
                continue;
            }
            let (x, y) = (coord.x as usize, coord.y as usize);
            let page = y / 8;
            let bit = y % 8;
            let idx = page * WIDTH as usize + x;
            match color {
                BinaryColor::On => self.buffer[idx] |= 1 << bit,
                BinaryColor::Off => self.buffer[idx] &= !(1 << bit),
            }
        }
        Ok(())
    }
}
