use std::{fs::{File, OpenOptions}, io::Write, time::{Duration, Instant}};
use crate::browser::Browser;

pub(crate) struct MemoryTrace {
    output: File,
    started: Instant,
    next: Instant,
}

impl MemoryTrace {
    pub(crate) fn from_environment() -> Option<Self> {
        let path = std::env::var_os("KAKANGKUNG_MEMORY_TRACE")?;
        match OpenOptions::new().create(true).append(true).open(path) {
            Ok(output) => Some(Self { output, started: Instant::now(), next: Instant::now() }),
            Err(error) => { eprintln!("memory trace: {error}"); None }
        }
    }

    pub(crate) fn tick(&mut self, browser: &Browser) -> Instant {
        if Instant::now() >= self.next {
            let mut snapshot = browser.memory_snapshot();
            snapshot["elapsed_ms"] = (self.started.elapsed().as_millis() as u64).into();
            if let Err(error) = writeln!(self.output, "{snapshot}") { eprintln!("memory trace: {error}"); }
            self.next = Instant::now() + Duration::from_secs(10);
        }
        self.next
    }
}
