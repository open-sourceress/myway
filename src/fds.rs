use log::warn;
use nix::{
	sys::{
		signal::{SigSet, Signal},
		signalfd::{signalfd, SfdFlags},
	},
	unistd::close,
};
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd};

/// An owned file descriptor.
///
/// The contained fd is not used except to call close(3) when the struct is dropped.
#[derive(Debug)]
pub struct Fd(RawFd);

impl AsRawFd for Fd {
	fn as_raw_fd(&self) -> RawFd {
		self.0
	}
}

impl IntoRawFd for Fd {
	fn into_raw_fd(self) -> RawFd {
		let fd = self.0;
		std::mem::forget(self);
		fd
	}
}

impl FromRawFd for Fd {
	unsafe fn from_raw_fd(fd: RawFd) -> Self {
		Self(fd)
	}
}

impl Drop for Fd {
	fn drop(&mut self) {
		match close(self.0) {
			Ok(()) => (),
			Err(err) => warn!("error closing {self:?}: {err:?}"),
		}
	}
}

/// Intercept SIGINT on the current thread, and return a file descriptor that will become readable when a signal is
/// caught.
///
/// The returned [`Fd`] is in nonblocking mode and should be registered with an [`Epoll`](crate::epoll::Epoll) with
/// interest `EPOLLIN` before use.
pub fn catch_sigint() -> nix::Result<Fd> {
	let mut signals = SigSet::empty();
	signals.add(Signal::SIGINT);
	signals.thread_block()?;
	let fd = signalfd(-1, &signals, SfdFlags::SFD_CLOEXEC | SfdFlags::SFD_NONBLOCK)?;
	Ok(Fd(fd))
}
