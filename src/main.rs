use self::{
	accept::Accept,
	client::Client,
	epoll::{Epoll, Event, EPOLLIN, EPOLLOUT},
	signals::catch_sigint,
};
use clap::Parser;
use log::{debug, info, trace, warn};
use slab::Slab;
use std::{
	io::{self, ErrorKind},
	path::PathBuf,
	task::Poll,
};

mod accept;
mod client;
mod epoll;
mod logging;
mod object_impls;
mod object_map;
mod protocol;
mod shm;
mod signals;
mod windows;

/// Wayland compositor
#[derive(Debug, Parser)]
struct CliArgs {
	/// Unix socket listener to bind on (default: $XDG_RUNTIME_DIR/wayland-0)
	#[clap(long)]
	socket_path: Option<PathBuf>,
}

/// Key (userdata) associated with the UnixListener in epoll
const ACCEPT_KEY: u64 = u64::MAX;
/// Key (userdata) associated with the signalfd in epoll
const SIGNAL_KEY: u64 = u64::MAX - 1;

fn main() -> io::Result<()> {
	env_logger::init();
	let CliArgs { socket_path } = CliArgs::parse();
	let socket_path = match socket_path {
		Some(path) => path,
		None => {
			let dir = std::env::var_os("XDG_RUNTIME_DIR")
				.ok_or_else(|| io::Error::new(ErrorKind::Other, "XDG_RUNTIME_DIR environment variable not set"))?;
			let mut path = PathBuf::from(dir);
			path.push("wayland-0");
			path
		},
	};
	let epoll = Epoll::new()?;

	info!("listening at {}", socket_path.display());
	let accept = Accept::bind(socket_path)?;
	epoll.register(&accept, EPOLLIN, ACCEPT_KEY)?;
	trace!("registered acceptor with epoll");

	let sigfd = catch_sigint()?;
	epoll.register(&sigfd, EPOLLIN, SIGNAL_KEY)?;
	trace!("registered signalfd with epoll");

	let mut clients = Slab::new();

	let mut events = [Event::empty(); 32];
	'run: loop {
		for event in epoll.wait_for_activity(&mut events, None)? {
			match event.data() {
				ACCEPT_KEY => {
					while let Poll::Ready(sock) = accept.poll_accept()? {
						let entry = clients.vacant_entry();
						let key = entry.key();
						epoll.register(&sock, EPOLLIN | EPOLLOUT, key as u64)?;
						trace!("registered socket with epoll (client key {key})");
						entry.insert(Client::new(sock));
						poll_client(&mut clients, key); // immediately poll until pending
					}
				},
				SIGNAL_KEY => break 'run,
				key => poll_client(&mut clients, key as usize),
			}
		}
	}

	debug!("exiting on SIGINT");
	Ok(())
}

fn poll_client(clients: &mut Slab<Client>, key: usize) {
	let client = match clients.get_mut(key) {
		Some(c) => c,
		None => {
			warn!("epoll produced unknown key {key}?");
			return;
		},
	};
	let (mut send, mut recv, objects) = client.split_mut();
	loop {
		let msg = match recv.poll_recv() {
			Poll::Ready(Ok(req)) => req,
			Poll::Ready(Err(err)) => {
				warn!("client {key} errored, dropping connection: {err:?}");
				clients.remove(key);
				return;
			},
			Poll::Pending => break,
		};
		match objects.dispatch_request(&mut send, msg) {
			Ok(()) => (),
			Err(err) => {
				warn!("client {key} errored, dropping connection: {err:?}");
				clients.remove(key);
				return;
			},
		}
	}
	trace!("flushing buffers");
	match send.poll_flush() {
		Poll::Ready(Ok(())) => (),
		Poll::Ready(Err(err)) => {
			warn!("client {key} errored, dropping connection: {err:?}");
			clients.remove(key);
		},
		Poll::Pending => (),
	}
}

fn cvt_poll<T, E: Into<io::Error>>(res: Result<T, E>) -> Poll<io::Result<T>> {
	match res.map_err(E::into) {
		Ok(x) => Poll::Ready(Ok(x)),
		Err(err) if err.kind() == ErrorKind::WouldBlock => Poll::Pending,
		Err(err) => Poll::Ready(Err(err)),
	}
}
