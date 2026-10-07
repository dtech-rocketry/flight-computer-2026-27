//! A small ring buffer of recent log lines, fed from a custom `log::Log`
//! implementation, so the OLED task can render a scrolling log window
//! alongside the normal serial log output.
//!
//! Replaces `esp_println::logger::init_logger_from_env()`: that reads the
//! `ESP_LOG` env var at *build* time (easy to silently build without it set
//! -- see git history), so this hardcodes the level instead.

use core::cell::RefCell;
use core::fmt::Write as _;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use heapless::{Deque, String, Vec};
use log::{Level, LevelFilter, Log, Metadata, Record};

pub const LOG_LINE_LEN: usize = 32;
pub const LOG_CAPACITY: usize = 6;

pub type LogLine = String<LOG_LINE_LEN>;

static LOG_BUFFER: Mutex<CriticalSectionRawMutex, RefCell<Deque<LogLine, LOG_CAPACITY>>> =
    Mutex::new(RefCell::new(Deque::new()));

struct RingLogger;

impl Log for RingLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        esp_println::println!("{} - {}", record.level(), record.args());

        let mut line: LogLine = String::new();
        let _ = write!(line, "{}", record.args());

        LOG_BUFFER.lock(|cell| {
            let mut buf = cell.borrow_mut();
            if buf.is_full() {
                buf.pop_front();
            }
            let _ = buf.push_back(line);
        });
    }

    fn flush(&self) {}
}

static LOGGER: RingLogger = RingLogger;

/// Installs the ring-buffer logger at `Info` level. Call once at startup in
/// place of `esp_println::logger::init_logger_from_env()`.
pub fn init() {
    unsafe {
        log::set_logger_racy(&LOGGER).ok();
        log::set_max_level_racy(LevelFilter::Info);
    }
}

/// Copies the current log lines (oldest first) into `out` for rendering,
/// without holding the lock during the (slow) I2C draw that follows.
pub fn snapshot(out: &mut Vec<LogLine, LOG_CAPACITY>) {
    out.clear();
    LOG_BUFFER.lock(|cell| {
        for line in cell.borrow().iter() {
            let _ = out.push(line.clone());
        }
    });
}
