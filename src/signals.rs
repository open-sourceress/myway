use nix::sys::{
	signal::{SigSet, Signal},
	signalfd::{signalfd, SfdFlags},
};
use std::os::unix::io::{FromRawFd, OwnedFd};

/// Intercept SIGINT on the current thread, and return a file descriptor that will become readable when a signal is
/// caught.
///
/// The returned [`Fd`] is in nonblocking mode and should be registered with an [`Epoll`](crate::epoll::Epoll) with
/// interest `EPOLLIN` before use.
pub fn catch_sigint() -> nix::Result<OwnedFd> {
	let mut signals = SigSet::empty();
	signals.add(Signal::SIGINT);
	signals.thread_block()?;
	let fd = signalfd(-1, &signals, SfdFlags::SFD_CLOEXEC | SfdFlags::SFD_NONBLOCK)?;
	// Safety: signalfd returns a new valid file descriptor which we immediately wrap
	Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}
