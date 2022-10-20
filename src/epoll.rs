use eyre::{Context, Result};
use nix::{
	sys::epoll::{epoll_create1, epoll_ctl, epoll_wait, EpollCreateFlags, EpollEvent, EpollFlags, EpollOp},
	unistd::close,
};
use std::{
	cell::Cell,
	io::{self, ErrorKind, Read, Write},
	os::unix::io::{AsRawFd, RawFd},
	time::Duration,
};

#[derive(Debug)]
pub struct Epoll {
	fd: RawFd,
	userdata: Cell<u64>,
}

impl Epoll {
	pub fn new() -> Result<Self> {
		Ok(Self {
			fd: epoll_create1(EpollCreateFlags::EPOLL_CLOEXEC).wrap_err("epoll_create failed")?,
			userdata: Cell::new(0),
		})
	}

	pub fn register<T: AsRawFd>(&self, io: T, flags: EpollFlags) -> Result<Polling<'_, T>> {
		let fd = io.as_raw_fd();
		let flags = flags | EpollFlags::EPOLLET;
		let ud = self.userdata.get();
		self.userdata.set(ud + 1);
		let mut event = Some(EpollEvent::new(flags, ud));
		epoll_ctl(self.fd, EpollOp::EpollCtlAdd, fd, &mut event)
			.wrap_err("epoll_ctl adding a file descriptor failed")?;
		println!("registered fd {} with userdata {}", fd, ud);
		Ok(Polling { epoll: self, flags: Cell::new(flags), io: Some(io), userdata: ud })
	}

	pub fn wait<'e>(&self, events: &'e mut [EpollEvent], timeout: Option<Duration>) -> Result<&'e mut [EpollEvent]> {
		let timeout = timeout.and_then(|t| t.as_millis().try_into().ok()).unwrap_or(-1);
		let n = epoll_wait(self.fd, events, timeout).wrap_err("epoll_wait failed")?;
		Ok(&mut events[..n])
	}

	fn unregister(&self, fd: RawFd) -> Result<()> {
		epoll_ctl(self.fd, EpollOp::EpollCtlDel, fd, &mut None)
			.wrap_err("epoll_ctl removing a file descriptor failed")?;
		Ok(())
	}
}

impl Drop for Epoll {
	fn drop(&mut self) {
		let _ = close(self.fd);
	}
}

#[derive(Debug)]
pub struct Polling<'e, T: AsRawFd> {
	epoll: &'e Epoll,
	flags: Cell<EpollFlags>,
	userdata: u64,
	io: Option<T>,
}

impl<'e, T: AsRawFd> Polling<'e, T> {
	pub fn get_ref(&self) -> &T {
		self.io.as_ref().unwrap()
	}

	#[allow(dead_code)]
	pub fn get_mut(&mut self) -> &mut T {
		self.io.as_mut().unwrap()
	}

	#[allow(dead_code)]
	pub fn into_inner(mut self) -> Result<T> {
		let io = self.io.take().unwrap();
		self.epoll.unregister(io.as_raw_fd())?;
		Ok(io)
	}

	pub fn reregister(&self, flags: EpollFlags) -> io::Result<()> {
		let mut event = Some(EpollEvent::new(flags, self.userdata));
		epoll_ctl(self.epoll.fd, EpollOp::EpollCtlMod, self.as_raw_fd(), &mut event)?;
		Ok(())
	}

	pub fn read_with<R>(&self, f: impl FnOnce(&T) -> io::Result<R>) -> io::Result<R> {
		let res = f(self.get_ref());
		if matches!(res, Err(ref err) if err.kind() == ErrorKind::WouldBlock) {
			self.reregister(self.flags.get() | EpollFlags::EPOLLIN)?;
		}
		res
	}

	pub fn read_with_mut<R>(&mut self, f: impl FnOnce(&mut T) -> io::Result<R>) -> io::Result<R> {
		let res = f(self.get_mut());
		if matches!(res, Err(ref err) if err.kind() == ErrorKind::WouldBlock) {
			self.reregister(self.flags.get() | EpollFlags::EPOLLIN)?;
		}
		res
	}

	pub fn write_with<R>(&self, f: impl FnOnce(&T) -> io::Result<R>) -> io::Result<R> {
		let res = f(self.get_ref());
		if matches!(res, Err(ref err) if err.kind() == ErrorKind::WouldBlock) {
			self.reregister(self.flags.get() | EpollFlags::EPOLLOUT)?;
		}
		res
	}

	pub fn write_with_mut<R>(&mut self, f: impl FnOnce(&mut T) -> io::Result<R>) -> io::Result<R> {
		let res = f(self.get_mut());
		if matches!(res, Err(ref err) if err.kind() == ErrorKind::WouldBlock) {
			self.reregister(self.flags.get() | EpollFlags::EPOLLOUT)?;
		}
		res
	}
}

impl<T: AsRawFd> AsRawFd for Polling<'_, T> {
	fn as_raw_fd(&self) -> RawFd {
		self.get_ref().as_raw_fd()
	}
}

impl<T: Read + AsRawFd> Read for Polling<'_, T> {
	fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
		self.read_with_mut(|f| f.read(buf))
	}
}

impl<T> Read for &Polling<'_, T>
where
	T: AsRawFd,
	for<'a> &'a T: Read,
{
	fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
		self.read_with(|mut f| f.read(buf))
	}
}

impl<T: Write + AsRawFd> Write for Polling<'_, T> {
	fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
		self.write_with_mut(|f| f.write(buf))
	}

	fn flush(&mut self) -> io::Result<()> {
		self.write_with_mut(|f| f.flush())
	}
}

impl<T> Write for &Polling<'_, T>
where
	T: AsRawFd,
	for<'a> &'a T: Write,
{
	fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
		self.write_with(|mut f| f.write(buf))
	}

	fn flush(&mut self) -> io::Result<()> {
		self.write_with(|mut f| f.flush())
	}
}

impl<'e, T: AsRawFd> Drop for Polling<'e, T> {
	fn drop(&mut self) {
		if let Some(io) = self.io.as_ref() {
			let _ = self.epoll.unregister(io.as_raw_fd());
		}
	}
}
