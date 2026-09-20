use std::{fs, io};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use crate::frame::LogFrame;

const LOGFILE_CAPACITY: usize = 5; //allow 5 log files before rewriting
const MEMORY_BUFFER_CAPACITY: usize = 100; //have up to 100 frames in memory
const FLUSH_INTERVAL: usize = 75; //flush buffer to file when buffer gets this full

fn init_logfile() -> io::Result<()> {
    let mut files: Vec<PathBuf> = fs::read_dir(".")?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let n = p.file_name().unwrap().to_str().unwrap();
            n.starts_with("flight_log_") && n.ends_with(".bin")
        })
        .collect();

    files.sort();

    // delete oldest files beyond capacity
    if files.len() > LOGFILE_CAPACITY {
        for f in &files[..files.len() - LOGFILE_CAPACITY] {
            fs::remove_file(f)?;
        }
    }

    Ok(())
}

fn push_file(frame_bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("flight_log_1.bin")?;

    file.write_all(frame_bytes)?;
    Ok(())
}

pub struct Logger {
    buffer: Vec<LogFrame>
}

impl Logger {
    pub fn new() -> Self {
        assert!(FLUSH_INTERVAL <= MEMORY_BUFFER_CAPACITY); // don't flush more frames than exist in memory
        Logger {
            buffer: Vec::with_capacity(MEMORY_BUFFER_CAPACITY)
        }
    }

    pub fn push(&mut self, lf: LogFrame) {
        self.buffer.push(lf);
        if self.buffer.len() >= FLUSH_INTERVAL {
            push_file(bytemuck::cast_slice(&self.buffer));
            self.buffer.clear();
        }
    }
}

