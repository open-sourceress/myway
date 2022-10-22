use crate::Fd;
use log::trace;
use nix::{
	sys::epoll::{epoll_create1, epoll_ctl, epoll_wait, EpollCreateFlags, EpollEvent, EpollFlags, EpollOp},
	Result,
};
use std::{
	os::unix::io::{AsRawFd, FromRawFd},
	time::Duration,
};

pub type Event = EpollEvent;

#[derive(Debug)]
pub struct Epoll {
	epfd: Fd,
}

impl Epoll {
	pub fn new() -> Result<Self> {
		let epfd = epoll_create1(EpollCreateFlags::EPOLL_CLOEXEC)?;
		// Safety: epoll_create1 returns a newly created file descriptor which we immediately wrap
		let epfd = unsafe { Fd::from_raw_fd(epfd) };
		trace!("created epollfd {epfd:?}");

		Ok(Self { epfd })
	}

	pub fn register(&self, fd: &impl AsRawFd, flags: Interest, key: u64) -> Result<()> {
		let fd = fd.as_raw_fd();
		epoll_ctl(
			self.epfd.as_raw_fd(),
			EpollOp::EpollCtlAdd,
			fd,
			&mut Some(EpollEvent::new(flags | EpollFlags::EPOLLET, key)),
		)?;
		trace!("registered fd {fd} with epoll");
		Ok(())
	}

	pub fn wait_for_activity<'e>(&self, events: &'e mut [Event], timeout: Option<Duration>) -> Result<&'e [Event]> {
		let timeout = timeout.map_or(-1, |d| d.as_millis() as _);
		trace!("> epoll_wait(epfd={}, events=[len={}], timeout_ms={timeout})", self.epfd.as_raw_fd(), events.len());
		let n = epoll_wait(self.epfd.as_raw_fd(), events, timeout)?;
		trace!("< {n}");
		Ok(&events[..n])
	}
}

pub type Interest = EpollFlags;
pub const EPOLLIN: Interest = EpollFlags::EPOLLIN;
pub const EPOLLOUT: Interest = EpollFlags::EPOLLOUT;
pub const EPOLLPRI: Interest = EpollFlags::EPOLLPRI;
pub const EPOLLERR: Interest = EpollFlags::EPOLLERR;
pub const EPOLLRDHUP: Interest = EpollFlags::EPOLLRDHUP;
pub const EPOLLHUP: Interest = EpollFlags::EPOLLHUP;
