use log::{debug, trace, warn};
use nix::{
	sys::{
		epoll::{epoll_create1, epoll_ctl, epoll_wait, EpollCreateFlags, EpollEvent, EpollFlags, EpollOp},
		signal::{SigSet, Signal},
		signalfd::{signalfd, SfdFlags},
	},
	unistd::close,
};
use slab::Slab;
use std::{
	fmt::{self, Debug, Formatter},
	io::{ErrorKind, Read, Result, Write},
	os::unix::{
		io::{AsRawFd, RawFd},
		net::{UnixListener, UnixStream},
	},
	path::Path,
	time::Duration,
};

/// Unix domain socket server that accepts connections on the wayland socket.
///
/// Internally, this server uses epoll(7) to manage many connections on a single thread.
#[derive(Debug)]
pub struct SocketServer {
	/// fd of an epoll used to listen on all sockets
	epoll: Fd,
	/// listener for accepting connections
	serv: UnixListener,
	/// signalfd for catching SIGINT (never actually read, just used to test for readability)
	_sigfd: Fd,
	/// client streams
	clients: Slab<ClientStream>,
}

impl SocketServer {
	const SERV_KEY: u64 = u64::MAX;
	const SIGNALFD_KEY: u64 = u64::MAX - 1;

	pub fn bind(path: impl AsRef<Path>) -> Result<Self> {
		let epoll = Fd(epoll_create1(EpollCreateFlags::EPOLL_CLOEXEC)?);
		trace!("created epollfd {epoll:?}");

		let serv = UnixListener::bind(path)?;
		serv.set_nonblocking(true)?;
		trace!("created server fd {serv:?}");
		epoll_ctl(
			epoll.as_raw_fd(),
			EpollOp::EpollCtlAdd,
			serv.as_raw_fd(),
			&mut Some(EpollEvent::new(EpollFlags::EPOLLIN | EpollFlags::EPOLLET, Self::SERV_KEY)),
		)?;
		trace!("registered server with epoll");

		let sigfd = {
			let mut signals = SigSet::empty();
			signals.add(Signal::SIGINT);
			signals.thread_block()?;
			Fd(signalfd(-1, &signals, SfdFlags::SFD_CLOEXEC | SfdFlags::SFD_NONBLOCK)?)
		};
		trace!("created signalfd {sigfd:?}");
		epoll_ctl(
			epoll.as_raw_fd(),
			EpollOp::EpollCtlAdd,
			sigfd.as_raw_fd(),
			&mut Some(EpollEvent::new(EpollFlags::EPOLLIN | EpollFlags::EPOLLET, Self::SIGNALFD_KEY)),
		)?;
		trace!("registered signalfd with epoll");

		let clients = Slab::new();

		Ok(Self { epoll, serv, _sigfd: sigfd, clients })
	}

	pub fn wait(&mut self, timeout: Option<Duration>) -> Result<bool> {
		let mut events = [EpollEvent::empty(); 32];
		let timeout = timeout.map_or(-1, |d| d.as_millis() as _);
		trace!(
			"calling epoll_wait(epfd={}, events=[len={}], timeout_ms={timeout})",
			self.epoll.as_raw_fd(),
			events.len()
		);
		let n = epoll_wait(self.epoll.as_raw_fd(), &mut events, timeout)?;
		trace!("epoll_wait returned {n}");
		for ev in &events[..n] {
			trace!("handling epoll event {{ events: {:?}, data: {:?} }}", ev.events(), ev.data());
			match ev.data() {
				Self::SERV_KEY => loop {
					match self.serv.accept() {
						Ok((sock, addr)) => {
							debug!("accepted socket {sock:?} from {addr:?}");
							sock.set_nonblocking(true)?;
							let fd = sock.as_raw_fd();
							let entry = self.clients.vacant_entry();
							let key = entry.key();
							trace!("client task key = {key}");
							epoll_ctl(
								self.epoll.as_raw_fd(),
								EpollOp::EpollCtlAdd,
								fd,
								&mut Some(EpollEvent::new(
									EpollFlags::EPOLLIN | EpollFlags::EPOLLOUT | EpollFlags::EPOLLET,
									key as u64,
								)),
							)?;
							trace!("registered socket with epoll");
							let client = entry.insert(ClientStream { sock, buf: Buffer::new() });
							trace!("put client into slab");
							match client.tick() {
								Ok(()) => {
									debug!("peer {key} closed connection");
									self.clients.remove(key as usize);
								},
								Err(err) if err.kind() == ErrorKind::WouldBlock => (),
								Err(err) => {
									warn!("task {key} failed: {err:?}");
									self.clients.remove(key as usize);
								},
							}
						},
						Err(err) if err.kind() == ErrorKind::WouldBlock => break,
						Err(err) => return Err(err),
					}
				},
				Self::SIGNALFD_KEY => return Ok(true),
				key => {
					if let Some(client) = self.clients.get_mut(key as usize) {
						match client.tick() {
							Ok(()) => {
								debug!("peer {key} closed connection");
								self.clients.remove(key as usize);
							},
							Err(err) if err.kind() == ErrorKind::WouldBlock => (),
							Err(err) => {
								warn!("task {key} failed: {err:?}");
								self.clients.remove(key as usize);
							},
						}
					} else {
						warn!("epoll_wait produced an event with unknown userdata {key}");
					}
				},
			}
		}
		Ok(false)
	}
}

impl Drop for SocketServer {
	fn drop(&mut self) {
		match self.serv.local_addr() {
			Ok(addr) => match addr.as_pathname() {
				Some(path) => match std::fs::remove_file(path) {
					Ok(()) => (),
					Err(err) => warn!("deleting server socket failed: {err:?}"),
				},
				None => warn!("deleting server socket failed: local_addr is not a pathname"),
			},
			Err(err) => warn!("deleting server socket failed: local_addr failed: {err:?}"),
		}
	}
}

/// An owned file descriptor.
///
/// The contained fd is not used except to call close(3) when the struct is dropped.
#[derive(Debug)]
struct Fd(RawFd);

impl AsRawFd for Fd {
	fn as_raw_fd(&self) -> RawFd {
		self.0
	}
}

impl Drop for Fd {
	fn drop(&mut self) {
		let _ = close(self.0);
	}
}

/// State associated with a SocketServer client
#[derive(Debug)]
struct ClientStream {
	/// Stream used to communicate with the client
	sock: UnixStream,
	/// Buffer of data read but not written back
	buf: Buffer,
}

impl ClientStream {
	fn tick(&mut self) -> Result<()> {
		loop {
			trace!("ticking {self:?}");
			while !self.buf.data().is_empty() {
				trace!("calling write(fd={}, buf=[len={}])", self.sock.as_raw_fd(), self.buf.data().len());
				let n = self.sock.write(self.buf.data())?;
				trace!("write returned {n}");
				if n == 0 {
					return Err(ErrorKind::WriteZero.into());
				}
				self.buf.mark_consumed(n);
			}
			trace!("calling read(fd={}, buf=[len={}])", self.sock.as_raw_fd(), self.buf.space_mut().len());
			let n = self.sock.read(self.buf.space_mut())?;
			trace!("read returned {n}");
			if n == 0 {
				break Ok(());
			}
			self.buf.mark_filled(n);
		}
	}
}

/// Ring buffer of bytes
#[derive(Clone)]
struct Buffer {
	/// heap-allocated buffer (initialized because working with uninit memory is unstable)
	bytes: Box<[u8; Self::CAPACITY]>,
	/// index of logically filled bytes to read from
	copyout_idx: usize,
	/// index of logically unfilled capacity to write into
	copyin_idx: usize,
}

impl Buffer {
	const CAPACITY: usize = 4096;

	fn new() -> Self {
		Self { bytes: Box::new([0; Self::CAPACITY]), copyout_idx: 0, copyin_idx: 0 }
	}

	fn data(&self) -> &[u8] {
		&self.bytes[self.copyout_idx..self.copyin_idx]
	}

	fn mark_consumed(&mut self, len: usize) {
		assert!(self.copyout_idx + len <= self.copyin_idx);
		self.copyout_idx += len;
		if self.copyout_idx == self.copyin_idx {
			self.copyout_idx = 0;
			self.copyin_idx = 0;
		}
	}

	fn space_mut(&mut self) -> &mut [u8] {
		&mut self.bytes[self.copyin_idx..]
	}

	fn mark_filled(&mut self, len: usize) {
		assert!(self.copyin_idx + len <= Self::CAPACITY);
		self.copyin_idx += len;
	}
}

impl Debug for Buffer {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.debug_struct("Buffer")
			.field("capacity", &Self::CAPACITY)
			.field("copyout_idx", &self.copyout_idx)
			.field("copyin_idx", &self.copyin_idx)
			.finish()
	}
}
