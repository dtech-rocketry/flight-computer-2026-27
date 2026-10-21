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
use heapless::{Deque, String};

use crate::pins;
use crate::power::Telemetry;

pub const WIDTH: u32 = 128;
pub const HEIGHT: u32 = 64;
const PAGES: usize = (HEIGHT / 8) as usize;
const BUFFER_LEN: usize = WIDTH as usize * PAGES;

/// Characters per scrollback line, based on FONT_6X10's 6px glyph width.
const LINE_WIDTH_CHARS: usize = (WIDTH / 6) as usize;

/// Vertical spacing between scrollback lines, matching the baseline step
/// `render` uses for its telemetry layout.
const LINE_HEIGHT: i32 = 11;

/// Scrollback lines that fit on screen at `LINE_HEIGHT` spacing.
const MAX_LINES: usize = (HEIGHT as i32 / LINE_HEIGHT) as usize;

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
    /// What was last actually written to the display, so `flush` can skip
    /// pages that haven't changed.
    shadow: [u8; BUFFER_LEN],
    /// Scrollback used by `push`, oldest line first.
    lines: Deque<String<LINE_WIDTH_CHARS>, MAX_LINES>,
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
            // Deliberately mismatched against the zeroed buffer so the
            // first flush() below writes the real clear to the display
            // instead of assuming it's already blank at power-on.
            shadow: [0xFF; BUFFER_LEN],
            lines: Deque::new(),
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

    /// Pushes changed pages (8 rows each) of the framebuffer to the
    /// display. Pages whose bytes match `shadow` since the last flush are
    /// skipped, so a small edit doesn't re-send the whole screen.
    pub async fn flush(&mut self) -> Result<(), E> {
        for page in 0..PAGES {
            let start = page * WIDTH as usize;
            let end = start + WIDTH as usize;

            if self.buffer[start..end] == self.shadow[start..end] {
                continue;
            }

            self.send_commands(&[
                0xB0 | page as u8,           // set page address
                COLUMN_OFFSET & 0x0F,        // set column address, low nibble
                0x10 | (COLUMN_OFFSET >> 4), // set column address, high nibble
            ])
            .await?;

            // Keep each I2C transaction's stack buffer small.
            let row = &self.buffer[start..end];
            for chunk in row.chunks(32) {
                let mut buf = heapless::Vec::<u8, 33>::new();
                buf.push(DATA_CONTROL_BYTE).ok();
                buf.extend_from_slice(chunk).ok();
                self.i2c.write(self.address, &buf).await?;
            }

            self.shadow[start..end].copy_from_slice(&self.buffer[start..end]);
        }
        Ok(())
    }

    /// Appends `text` to the on-screen scrollback: word-wraps it to the
    /// display width, drops the oldest line once the screen is full, then
    /// redraws and flushes only the display pages that actually changed.
    ///
    /// This is a separate "log view" from `render`'s telemetry layout --
    /// calling one after the other will overwrite what the other drew.
    pub async fn push(&mut self, text: &str) -> Result<(), E> {
        let mut line: String<LINE_WIDTH_CHARS> = String::new();

        for word in text.split_ascii_whitespace() {
            let separator_len = if line.is_empty() { 0 } else { 1 };
            if !line.is_empty() && line.len() + separator_len + word.len() > LINE_WIDTH_CHARS {
                self.push_line(line);
                line = String::new();
            }

            // Words longer than a full line get hard-broken across
            // several lines instead of overflowing one.
            let mut remaining = word;
            while remaining.len() > LINE_WIDTH_CHARS {
                let (chunk, rest) = remaining.split_at(LINE_WIDTH_CHARS);
                let mut chunk_line: String<LINE_WIDTH_CHARS> = String::new();
                let _ = chunk_line.push_str(chunk);
                self.push_line(chunk_line);
                remaining = rest;
            }

            if !line.is_empty() {
                let _ = line.push(' ');
            }
            let _ = line.push_str(remaining);
        }

        if !line.is_empty() {
            self.push_line(line);
        }

        self.redraw_lines();
        self.flush().await
    }

    /// Adds one wrapped line to the scrollback, evicting the oldest line
    /// first if the screen is already full.
    fn push_line(&mut self, line: String<LINE_WIDTH_CHARS>) {
        if self.lines.is_full() {
            self.lines.pop_front();
        }
        let _ = self.lines.push_back(line);
    }

    /// Redraws every line in the scrollback into `buffer` at its current
    /// position. Doesn't touch the display -- call `flush` afterward.
    fn redraw_lines(&mut self) {
        self.clear();

        // Clone the lines out first: drawing needs `&mut self`, which
        // can't happen while `self.lines` is still borrowed by the loop.
        let lines: heapless::Vec<String<LINE_WIDTH_CHARS>, MAX_LINES> =
            self.lines.iter().cloned().collect();

        for (i, line) in lines.iter().enumerate() {
            let y = i as i32 * LINE_HEIGHT;
            let _ = Text::with_baseline(line, Point::new(0, y), TEXT_STYLE, Baseline::Top)
                .draw(self);
        }
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
