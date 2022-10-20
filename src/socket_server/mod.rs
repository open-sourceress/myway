use self::buffer::Buffer;
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
	io::{ErrorKind, Read, Result, Write},
	os::unix::{
		io::{AsRawFd, RawFd},
		net::{UnixListener, UnixStream},
	},
	path::Path,
	time::Duration,
};

mod buffer;

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
					Ok(()) => debug!("deleted server socket at {path:?}"),
					Err(err) => warn!("deleting server socket failed: {err:?}"),
				},
				None => warn!("deleting server socket failed: local_addr ({addr:?}) is not a pathname"),
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
		{
			let mut space = self.buf.byte_space_mut();
			while !space.is_empty() {
				trace!("calling read(fd={}, buf=[len={}])", self.sock.as_raw_fd(), space.len());
				let n = match self.sock.read(space) {
					Ok(0) => return Ok(()), // TODO handle half-shutdown, is that a thing Unix sockets can do?
					Ok(n) => n,
					Err(err) if err.kind() == ErrorKind::WouldBlock => break,
					Err(err) => return Err(err),
				};
				trace!("read returned {n}");
				self.buf.mark_bytes_filled(n);
				space = self.buf.byte_space_mut();
			}
		}
		{
			let mut data = self.buf.byte_data();
			while !data.is_empty() {
				trace!("calling write(fd={}, buf=[len={}])", self.sock.as_raw_fd(), data.len());
				let n = match self.sock.write(data) {
					Ok(0) => return Err(ErrorKind::WriteZero.into()),
					Ok(n) => n,
					Err(err) if err.kind() == ErrorKind::WouldBlock => break,
					Err(err) => return Err(err),
				};
				trace!("write returned {n}");
				self.buf.mark_bytes_consumed(n);
				data = self.buf.byte_data();
			}
		}
		Err(ErrorKind::WouldBlock.into())
	}
}