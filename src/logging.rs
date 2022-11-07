use once_cell::sync::Lazy;
use std::{
	cell::Cell,
	env,
	fmt::{Debug, Write as _},
	io::{stderr, Write as _},
	num::NonZeroU32,
	os::unix::io::AsRawFd,
	time::SystemTime,
};

static WAYLAND_DEBUG: Lazy<bool> = Lazy::new(|| matches!(env::var("WAYLAND_DEBUG").as_deref(), Ok("1" | "server")));

thread_local! {
	/// A reused buffer for building logs. Each log line is written in parts to this buffer before being emitted as a complete line to stderr.
	///
	/// Instead of requiring a separate `impl FnOnce` for every request and event to call in `LocalKey::with`, we take the buffer out and put it back when we're done. In case the buffer doesn't get put back for some reason, a usable but empty string is left in its place.
	static BUFFER: Cell<String> = Cell::default();
}

pub fn log_request(interface_name: &'static str, request_name: &'static str, object_id: u32) -> Option<LogMessage> {
	log_message("", interface_name, request_name, object_id)
}

pub fn log_event(interface_name: &'static str, event_name: &'static str, object_id: u32) -> Option<LogMessage> {
	log_message(" -> ", interface_name, event_name, object_id)
}

fn log_message(
	prefix: &'static str,
	interface_name: &'static str,
	message_name: &'static str,
	object_id: u32,
) -> Option<LogMessage> {
	if !*WAYLAND_DEBUG {
		return None;
	}

	let mut buffer = BUFFER.with(|cell| cell.take());
	buffer.clear();

	if let Ok(time) = SystemTime::UNIX_EPOCH.elapsed() {
		// libwayland truncates microseconds (tv_sec*1e9 + tv_nsec/1000) to an int, then formats as
		// printf("%7u.%03u", micros / 1000, micros % 1000)
		let micros = time.as_micros() as u32;
		let _ = write!(buffer, "[{:>7}.{:>03}]", micros / 1000, micros % 1000);
	} else {
		// before 1970 somehow? print an error
		buffer.push_str("[???????.???]");
	}
	let _ = write!(buffer, " {prefix}{interface_name}@{object_id}.{message_name}(");
	Some(LogMessage { buffer })
}

pub struct LogMessage {
	buffer: String,
}

impl LogMessage {
	pub fn arg_debug(&mut self, arg: impl Debug) {
		let _ = write!(self.buffer, "{arg:?}, ");
	}

	#[allow(dead_code)]
	pub fn arg_nil(&mut self) {
		self.buffer.push_str("nil, ");
	}

	pub fn arg_object(&mut self, interface: Option<&'static str>, id: NonZeroU32) {
		let _ = write!(self.buffer, "{}@{id}, ", interface.unwrap_or("[unknown]"));
	}

	pub fn arg_new_id(&mut self, interface: Option<&'static str>, id: NonZeroU32) {
		let _ = write!(self.buffer, "new id {}@{id}, ", interface.unwrap_or("[unknown]"));
	}

	#[allow(dead_code)]
	pub fn arg_array(&mut self, arg: &[u32]) {
		let _ = write!(self.buffer, "array[{}], ", arg.len());
	}

	pub fn arg_fd(&mut self, arg: &impl AsRawFd) {
		let _ = write!(self.buffer, "fd {}, ", arg.as_raw_fd());
	}

	pub fn finish(mut self) {
		if self.buffer.ends_with(", ") {
			self.buffer.truncate(self.buffer.len() - 2);
		}
		self.buffer.push_str(")\n");
		let _ = stderr().lock().write_all(self.buffer.as_bytes());
	}
}

impl Drop for LogMessage {
	fn drop(&mut self) {
		let buffer = std::mem::take(&mut self.buffer);
		BUFFER.with(|cell| cell.set(buffer));
	}
}
