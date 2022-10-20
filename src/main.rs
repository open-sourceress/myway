use self::socket_server::SocketServer;
use clap::Parser;
use log::{debug, info, trace};
use std::{
	io::{self, ErrorKind},
	path::PathBuf,
};

mod socket_server;

/// Wayland compositor
#[derive(Debug, Parser)]
struct Args {
	/// Unix socket listener to bind on (default: $XDG_RUNTIME_DIR/wayland-0)
	#[clap(long)]
	socket_path: Option<PathBuf>,
}

fn main() -> io::Result<()> {
	log::set_boxed_logger(Box::new(Logger(io::stderr()))).expect("logger should be set in main");
	log::set_max_level(Logger::MAX_LEVEL);
	let Args { socket_path } = Args::parse();
	let socket_path = match socket_path {
		Some(path) => path,
		None => {
			let dir = std::env::var_os("XDG_RUNTIME_DIR")
				.ok_or_else(|| io::Error::new(ErrorKind::Other, "XDG_RUNTIME_DIR environment variable not set"))?;
			let mut path = PathBuf::from(dir);
			path.push("wayland-0");
			path
		},
	};
	info!("binding to {socket_path:?}");
	let mut server = SocketServer::bind(socket_path)?;
	loop {
		trace!("ticking socket server");
		if server.wait(None)? {
			break;
		}
	}
	debug!("exiting on SIGINT");
	Ok(())
}

struct Logger(io::Stderr);

#[cfg(debug_assertions)]
impl Logger {
	const MAX_LEVEL: log::LevelFilter = log::LevelFilter::Trace;
}

#[cfg(not(debug_assertions))]
impl Logger {
	const MAX_LEVEL: log::LevelFilter = log::LevelFilter::Info;
}

impl log::Log for Logger {
	fn enabled(&self, metadata: &log::Metadata) -> bool {
		metadata.level() <= Self::MAX_LEVEL
	}

	fn log(&self, record: &log::Record) {
		use std::io::{LineWriter, Write as _};
		if !self.enabled(record.metadata()) {
			return;
		}
		let mut dest = LineWriter::new(self.0.lock());
		let _ = writeln!(dest, "[{level:<5}] {args}", level = record.level(), args = record.args());
		let _ = dest.flush();
	}

	fn flush(&self) {
		use std::io::Write as _;
		let _ = (&self.0).flush();
	}
}
