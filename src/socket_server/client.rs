use super::buffer::Buffer;
use log::trace;
use std::{
	io::{ErrorKind, Read, Result, Write},
	os::unix::{io::AsRawFd, net::UnixStream},
};

/// State associated with a SocketServer client
#[derive(Debug)]
pub(crate) struct ClientStream {
	/// Stream used to communicate with the client
	sock: UnixStream,
	/// Buffer of events and errors to send to the client
	tx_buf: Buffer,
	/// Buffer of requests read from the client
	rx_buf: Buffer,
}

impl ClientStream {
	pub(super) fn new(sock: UnixStream) -> Self {
		Self { sock, tx_buf: Buffer::new(), rx_buf: Buffer::new() }
	}

	pub(crate) fn maintain(&mut self) -> Result<()> {
		{
			let mut space = self.rx_buf.byte_space_mut();
			while !space.is_empty() {
				trace!("calling read(fd={}, buf=[len={}])", self.sock.as_raw_fd(), space.len());
				let n = match self.sock.read(space) {
					Ok(0) => return Ok(()), // TODO handle half-shutdown, is that a thing Unix sockets can do?
					Ok(n) => n,
					Err(err) if err.kind() == ErrorKind::WouldBlock => break,
					Err(err) => return Err(err),
				};
				trace!("read returned {n}");
				self.rx_buf.mark_bytes_filled(n);
				space = self.rx_buf.byte_space_mut();
			}
		}
		{
			let mut data = self.tx_buf.byte_data();
			while !data.is_empty() {
				trace!("calling write(fd={}, buf=[len={}])", self.sock.as_raw_fd(), data.len());
				let n = match self.sock.write(data) {
					Ok(0) => return Err(ErrorKind::WriteZero.into()),
					Ok(n) => n,
					Err(err) if err.kind() == ErrorKind::WouldBlock => break,
					Err(err) => return Err(err),
				};
				trace!("write returned {n}");
				self.tx_buf.mark_bytes_consumed(n);
				data = self.tx_buf.byte_data();
			}
		}
		Err(ErrorKind::WouldBlock.into())
	}

	pub(crate) fn read_message(&mut self) -> Option<(u32, u16, &[u32])> {
		self.rx_buf.read_message()
	}
}
