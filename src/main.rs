use self::socket_server::SocketServer;
use clap::Parser;
use log::debug;
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
	log::set_max_level(log::LevelFilter::Trace);
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
	debug!("binding to {socket_path:?}");
	let mut server = SocketServer::bind(socket_path)?;
	loop {
		debug!("ticking socket server");
		if server.wait(None)? {
			break;
		}
	}
	debug!("exiting on SIGINT");
	Ok(())
}

struct Logger(io::Stderr);

impl log::Log for Logger {
	fn enabled(&self, metadata: &log::Metadata) -> bool {
		metadata.level() <= log::Level::Debug
	}

	fn log(&self, record: &log::Record) {
		use std::io::Write as _;
		if !self.enabled(record.metadata()) {
			return;
		}
		let mut dest = self.0.lock();
		let _ = writeln!(dest, "[{level}] {args}", level = record.level(), args = record.args());
	}

	fn flush(&self) {
		use std::io::Write as _;
		let _ = (&self.0).flush();
	}
}
