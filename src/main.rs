use self::epoll::Epoll;
use clap::Parser;
use epoll::Polling;
use eyre::{bail, eyre, Result, WrapErr};
use nix::sys::{
	epoll::{EpollEvent, EpollFlags},
	signal::{SigSet, Signal},
	signalfd::{signalfd, SfdFlags},
};
use slab::Slab;
use std::{
	fs::File,
	io::{self, ErrorKind, Read, Write},
	os::unix::{
		net::{UnixListener, UnixStream},
		prelude::FromRawFd,
	},
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
	let listener =
		epoll.register(listener, EpollFlags::EPOLLIN, u64::MAX).wrap_err("registering socket listener failed")?;

	let mut sigfd = {
		let mut signals = SigSet::empty();
		signals.add(Signal::SIGINT);
		signals.thread_block()?;
		let fd = signalfd(-1, &signals, SfdFlags::SFD_CLOEXEC | SfdFlags::SFD_NONBLOCK)?;
		// Safety: just something that can close the fd on exit, never used
		let file = unsafe { File::from_raw_fd(fd) };
		epoll.register(file, EpollFlags::EPOLLIN, u64::MAX - 1)?
	};

	let mut tasks = Slab::new();
	loop {
		match listener.read_with(|l| l.accept()) {
			Ok((sock, _)) => {
				let entry = tasks.vacant_entry();
				if let Ok(task) = ClientTask::new(&epoll, sock, entry.key()) {
					let _ = entry.insert(task).tick();
				}
			},
			Err(err) if err.kind() == ErrorKind::WouldBlock => (),
			Err(err) => bail!("accepting connection failed: {err}"),
		}
		match sigfd.read(&mut [0; 128]) {
			Ok(_) => break,
			Err(err) if err.kind() == ErrorKind::WouldBlock => (),
			Err(err) => bail!(err),
		};
		let mut events = [EpollEvent::empty(); 10];
		let events = epoll.wait(&mut events, None)?;
		for event in events {
			if let Some(task) = tasks.get_mut(event.data() as usize) {
				match task.tick() {
					Ok(()) => {
						tasks.remove(event.data() as usize);
					},
					Err(err) if err.kind() == ErrorKind::WouldBlock => (),
					Err(err) => bail!(err),
				}
			}
		}
	}

	println!("exiting on ^C");
	let _ = std::fs::remove_file(&socket_path);
	Ok(())
}

#[derive(Debug)]
struct ClientTask<'e> {
	sock: Polling<'e, UnixStream>,
	tx_buf: Vec<u8>,
}

impl<'e> ClientTask<'e> {
	fn new(epoll: &'e Epoll, sock: UnixStream, key: usize) -> Result<Self> {
		sock.set_nonblocking(true).wrap_err("setting socket to nonblocking failed")?;
		let sock = epoll.register(sock, EpollFlags::EPOLLIN, key as u64)?;
		let mut tx_buf = Vec::with_capacity(4096);
		tx_buf.spare_capacity_mut().iter_mut().for_each(|byte| {
			byte.write(0);
		});
		Ok(Self { sock, tx_buf: Vec::with_capacity(4096) })
	}

	fn tick(&mut self) -> io::Result<()> {
		let mut sock = &self.sock;
		loop {
			while !self.tx_buf.is_empty() {
				let n = sock.write(&self.tx_buf)?;
				if n == 0 {
					return Err(ErrorKind::WriteZero.into());
				}
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
