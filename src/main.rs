use self::epoll::Epoll;
use clap::Parser;
use epoll::Polling;
use eyre::{bail, ensure, eyre, Result, WrapErr};
use nix::sys::epoll::{EpollEvent, EpollFlags};
use std::{
	io::{self, ErrorKind, Read, Write},
	os::unix::net::{UnixListener, UnixStream},
	path::PathBuf,
	slice::from_raw_parts_mut,
};

mod epoll;

/// Wayland compositor
#[derive(Debug, Parser)]
struct Args {
	/// Unix socket listener to bind on (default: $XDG_RUNTIME_DIR/wayland-0)
	#[clap(long)]
	socket_path: Option<PathBuf>,
}

fn main() -> Result<()> {
	let Args { socket_path } = Args::parse();
	let socket_path = match socket_path {
		Some(path) => path,
		None => {
			let dir = std::env::var_os("XDG_RUNTIME_DIR")
				.ok_or_else(|| eyre!("XDG_RUNTIME_DIR environment variable not set"))?;
			let mut path = PathBuf::from(dir);
			path.push("wayland-0");
			path
		},
	};

	let epoll = Epoll::new()?;

	let listener =
		UnixListener::bind(&socket_path).wrap_err_with(|| format!("binding to {} failed", socket_path.display()))?;
	listener.set_nonblocking(true).wrap_err("setting socket to nonblocking failed")?;
	let listener = epoll.register(listener, EpollFlags::EPOLLIN).wrap_err("registering socket listener failed")?;

	let mut tasks = Vec::new();
	loop {
		println!("ticking tasks");
		match listener.read_with(|l| l.accept()) {
			Ok((sock, addr)) => {
				println!("accepted connection from {addr:?}");
				if let Ok(task) = ClientTask::new(&epoll, sock) {
					tasks.push(task);
				}
			},
			Err(err) if err.kind() == ErrorKind::WouldBlock => (),
			Err(err) => bail!("accepting connection failed: {err}"),
		}
		for i in (0..tasks.len()).rev() {
			let task = &mut tasks[i];
			match task.tick() {
				Ok(()) => {
					println!("peer disconnected");
					tasks.swap_remove(i);
				},
				Err(err) if err.downcast_ref::<io::Error>().map(|err| err.kind()) == Some(ErrorKind::WouldBlock) => {
					continue
				},
				Err(err) => {
					println!("task failed!\n{err:?}");
					tasks.swap_remove(i);
				},
			}
		}
		println!("calling epoll_wait");
		let mut events = [EpollEvent::empty(); 10];
		let events = epoll.wait(&mut events, None)?;
		println!("epoll_wait returned {} events: {:?}", events.len(), events);
		println!(
			"EPOLLIN={:?}, EPOLLOUT={:?}, EPOLLERR={:?}",
			EpollFlags::EPOLLIN.bits(),
			EpollFlags::EPOLLOUT.bits(),
			EpollFlags::EPOLLERR.bits(),
		);
	}

	// let _ = std::fs::remove_file(&socket_path)?;
}

#[derive(Debug)]
struct ClientTask<'e> {
	sock: Polling<'e, UnixStream>,
	tx_buf: Vec<u8>,
}

impl<'e> ClientTask<'e> {
	fn new(epoll: &'e Epoll, sock: UnixStream) -> Result<Self> {
		sock.set_nonblocking(true).wrap_err("setting socket to nonblocking failed")?;
		let sock = epoll.register(sock, EpollFlags::EPOLLIN)?;
		let mut tx_buf = Vec::with_capacity(4096);
		tx_buf.spare_capacity_mut().iter_mut().for_each(|byte| {
			byte.write(0);
		});
		Ok(Self { sock, tx_buf: Vec::with_capacity(4096) })
	}

	fn tick(&mut self) -> Result<()> {
		let mut sock = &self.sock;
		loop {
			while !self.tx_buf.is_empty() {
				let n = sock.write(&self.tx_buf)?;
				ensure!(n > 0, io::Error::from(ErrorKind::WriteZero));
				self.tx_buf.drain(..n);
			}
			let space = self.tx_buf.spare_capacity_mut();
			let space = unsafe { from_raw_parts_mut(space.as_mut_ptr().cast(), space.len()) };
			let n = sock.read(space)?;
			if n == 0 {
				break Ok(());
			}
			unsafe {
				self.tx_buf.set_len(n);
			}
		}
	}
}
