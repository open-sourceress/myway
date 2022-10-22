use log::{LevelFilter, Log, Metadata, Record};
use std::io::{stderr, LineWriter, Stderr, Write as _};

pub fn init() {
	log::set_boxed_logger(Box::new(Logger(stderr()))).unwrap();
	log::set_max_level(MAX_LEVEL);
}

struct Logger(Stderr);

#[cfg(debug_assertions)]
const MAX_LEVEL: LevelFilter = LevelFilter::Trace;
#[cfg(not(debug_assertions))]
const MAX_LEVEL: LevelFilter = LevelFilter::Info;

impl Log for Logger {
	fn enabled(&self, metadata: &Metadata) -> bool {
		metadata.level() <= MAX_LEVEL
	}

	fn log(&self, record: &Record) {
		if !self.enabled(record.metadata()) {
			return;
		}
		let mut dest = LineWriter::new(self.0.lock());
		let _ = writeln!(dest, "[{level:>5}] {args}", level = record.level(), args = record.args());
		let _ = dest.flush();
	}

	fn flush(&self) {
		let _ = (&self.0).flush();
	}
}
