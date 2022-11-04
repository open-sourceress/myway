use log::trace;
use nix::{
	sys::epoll::{epoll_create1, epoll_ctl, epoll_wait, EpollCreateFlags, EpollEvent, EpollFlags, EpollOp},
	Result,
};
use std::{
	os::unix::io::{AsRawFd, FromRawFd, OwnedFd},
	time::Duration,
};

pub type Event = EpollEvent;

#[derive(Debug)]
pub struct Epoll {
	epfd: OwnedFd,
}

impl Epoll {
	pub fn new() -> Result<Self> {
		let epfd = epoll_create1(EpollCreateFlags::EPOLL_CLOEXEC)?;
		// Safety: epoll_create1 returns a newly created file descriptor which we immediately wrap
		let epfd = unsafe { OwnedFd::from_raw_fd(epfd) };
		trace!("created epollfd {epfd:?}");
		Ok(Self { epfd })
	}

	pub fn register(&self, fd: &impl AsRawFd, flags: Interest, key: u64) -> Result<()> {
		let epfd = self.epfd.as_raw_fd();
		let fd = fd.as_raw_fd();
		epoll_ctl(epfd, EpollOp::EpollCtlAdd, fd, &mut Some(EpollEvent::new(flags | EpollFlags::EPOLLET, key)))?;
		trace!("registered fd {fd} with epoll {epfd}");
		Ok(())
	}

	pub fn wait_for_activity<'e>(&self, events: &'e mut [Event], timeout: Option<Duration>) -> Result<&'e [Event]> {
		let timeout = timeout.map_or(-1, |d| d.as_millis() as _);
		let n = epoll_wait(self.epfd.as_raw_fd(), events, timeout)?;
		Ok(&events[..n])
	}
}

pub type Interest = EpollFlags;
pub const EPOLLIN: Interest = EpollFlags::EPOLLIN;
pub const EPOLLOUT: Interest = EpollFlags::EPOLLOUT;
// pub const EPOLLPRI: Interest = EpollFlags::EPOLLPRI;
// pub const EPOLLERR: Interest = EpollFlags::EPOLLERR;
// pub const EPOLLRDHUP: Interest = EpollFlags::EPOLLRDHUP;
// pub const EPOLLHUP: Interest = EpollFlags::EPOLLHUP;
