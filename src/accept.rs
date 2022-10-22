use crate::cvt_poll;
use log::{debug, trace, warn};
use std::{
	fs,
	io::Result,
	os::unix::{
		io::{AsRawFd, RawFd},
		net::{UnixListener, UnixStream},
	},
	path::Path,
	task::{ready, Poll},
};

/// Unix domain socket listener that accepts connections on the wayland socket.
///
/// Register with an [`Epoll`](crate::epoll::Epoll) before use.
#[derive(Debug)]
pub struct Accept {
	listener: UnixListener,
}

impl Accept {
	/// Create a new acceptor listening on the given socket path.
	///
	/// Before using, register with an [`Epoll`](crate::epoll::Epoll) with interest `EPOLLIN`.
	pub fn bind(path: impl AsRef<Path>) -> Result<Self> {
		let lst = UnixListener::bind(path)?;
		lst.set_nonblocking(true)?;
		trace!("created listener {lst:?}");
		Ok(Self { listener: lst })
	}

	/// Accept a waiting connection, if any.
	///
	/// The returned socket is in nonblocking mode and should be registered with an [`Epoll`](crate::epoll::Epoll)
	/// before use.
	pub fn poll_accept(&self) -> Poll<Result<UnixStream>> {
		let (sock, _) = ready!(cvt_poll(self.listener.accept()))?;
		debug!("accepted connection {sock:?}"); // {sock:?} includes local and peer addrs
		sock.set_nonblocking(true)?;
		Poll::Ready(Ok(sock))
	}
}

impl AsRawFd for Accept {
	fn as_raw_fd(&self) -> RawFd {
		self.listener.as_raw_fd()
	}
}

impl Drop for Accept {
	fn drop(&mut self) {
		match self.listener.local_addr() {
			Ok(addr) => match addr.as_pathname() {
				Some(path) => match fs::remove_file(path) {
					Ok(()) => debug!("deleted server socket at {path:?}"),
					Err(err) => warn!("deleting server socket failed: {err:?}"),
				},
				None => warn!("deleting server socket failed: local_addr ({addr:?}) is not a pathname"),
			},
			Err(err) => warn!("deleting server socket failed: local_addr failed: {err:?}"),
		}
	}
}
