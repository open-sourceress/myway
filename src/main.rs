use self::{
	accept::Accept,
	client::Client,
	epoll::{Epoll, Event, EPOLLIN, EPOLLOUT},
	fds::{catch_sigint, Fd},
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
mod fds;
mod logger;
mod protocol;

/// Wayland compositor
#[derive(Debug, Parser)]
struct Args {
	/// Unix socket listener to bind on (default: $XDG_RUNTIME_DIR/wayland-0)
	#[clap(long)]
	socket_path: Option<PathBuf>,
}

/// Key (userdata) associated with the UnixListener in epoll
const ACCEPT_KEY: u64 = u64::MAX;
/// Key (userdata) associated with the signalfd in epoll
const SIGNAL_KEY: u64 = u64::MAX - 1;

fn main() -> io::Result<()> {
	logger::init();
	let Args { socket_path } = Args::parse();
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
	let mut event_serial = 0;
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
						poll_client(&mut clients, &mut event_serial, key); // immediately poll until pending
					}
				},
				SIGNAL_KEY => break 'run,
				key => poll_client(&mut clients, &mut event_serial, key as usize),
			}
		}
	}

	debug!("exiting on SIGINT");
	Ok(())
}

macro_rules! wire_event {
	(id = $id:expr, op = $op:expr) => { wire_event![id=$id, op=$op;]};
	(id = $id:expr, op = $op:expr; $($arg:expr),* $(,)?) => {
		{
			let mut words = [$id, $op, $($arg,)*];
			words[1] |= (words.len() as u32) << 18;
			words
		}
	}
}

fn poll_client(clients: &mut Slab<Client>, event_serial: &mut u32, key: usize) {
	let client = match clients.get_mut(key) {
		Some(c) => c,
		None => {
			warn!("epoll produced unknown key {key}?");
			return;
		},
	};
	loop {
		let (oid, op, args) = match client.poll_recv() {
			Poll::Ready(Ok(req)) => req,
			Poll::Ready(Err(err)) => {
				warn!("client {key} errored, dropping connection: {err:?}");
				clients.remove(key);
				return;
			},
			Poll::Pending => break,
		};
		info!("message for {oid}! opcode={op}, args={args:?}");
		match (oid, op, args) {
			(1, 0, &[cb_id]) => {
				info!("wl_display::sync(callback={cb_id})");
				let serial = *event_serial;
				*event_serial += 1;
				client.submit(&wire_event![id=cb_id, op=0; serial]).unwrap();
			},
			(1, 1, &[reg_id]) => {
				info!("wl_display::get_registry(registry={reg_id})");
				// issue registry::global (op 0) events for some made-up globals
				// args: name:uint, interface:string, version:uint
				client
					.submit(&wire_event![
						id=reg_id, op=0;
						0, // name: uint
						14, // interface: string (len)
						u32::from_ne_bytes(*b"wl_c"),
						u32::from_ne_bytes(*b"ompo"),
						u32::from_ne_bytes(*b"sito"),
						u32::from_ne_bytes(*b"r\0\0\0"),
						5, // version: uint
					])
					.unwrap();
				client
					.submit(&wire_event![
						id=reg_id, op=0;
						1, // name: uint
						7, // interface: string (len)
						u32::from_ne_bytes(*b"wl_s"),
						u32::from_ne_bytes(*b"hm\0\0"),
						1, // version: uint
					])
					.unwrap();
				client
					.submit(&wire_event![
						id=reg_id, op=0;
						2, // name: uint
						23, // interface: string (len)
						u32::from_ne_bytes(*b"wl_d"),
						u32::from_ne_bytes(*b"ata_"),
						u32::from_ne_bytes(*b"devi"),
						u32::from_ne_bytes(*b"ce_m"),
						u32::from_ne_bytes(*b"anag"),
						u32::from_ne_bytes(*b"er\0\0"),
						3, // version: uint
					])
					.unwrap();
			},
			_ => (),
		};
	}
	trace!("flushing buffers");
	match client.poll_flush() {
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
