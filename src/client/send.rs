use super::{div_exact, Buffer, FdBuffer, CAP_BYTES, CAP_FDS};
use crate::{
	cvt_poll,
	protocol::{AnyObject, Id, Word, WORD_SIZE},
};
use log::trace;
use nix::sys::socket::{sendmsg, ControlMessage, MsgFlags};
use std::{
	io::{Error, ErrorKind, IoSlice, Result},
	os::unix::{io::AsRawFd, net::UnixStream},
	task::{ready, Poll},
};

#[derive(Debug)]
pub struct SendHalf<'c> {
	pub(super) sock: &'c UnixStream,
	pub(super) bytes: &'c mut Buffer,
	pub(super) fds: &'c mut FdBuffer,
}

impl<'c> SendHalf<'c> {
	/// Queue a message to be sent to this peer.
	///
	/// `object_id` and `opcode` are included in the message header verbatim. `args_len` and `fds_len` count the
	/// protocol words and file descriptors included in this message's arguments, respectively. **Note:** `args_len`
	/// differs from the message header's length field, which includes the length of the header and counts in bytes.
	///
	/// Callers must ensure that exactly `args_len` words of data and `fds_len` file descriptors are encoded into the
	/// returned [`SendMessage`], and then call its [`finish`](SendMessage::finish) method. Writing too much or too
	/// little discards the message and may panic. Dropping or leaking the `SendMessage` without calling `finish`
	/// discards the message but otherwise leaves this `SendHalf` in a consistent state. At no point is the message
	/// partially delivered.
	pub fn submit(
		&mut self,
		object_id: Id<AnyObject>,
		opcode: u16,
		args_len: usize,
		fds_len: usize,
	) -> Result<SendMessage<'_>> {
		let words_len = args_len + 2;
		let bytes_len = words_len * WORD_SIZE;
		assert!(bytes_len <= CAP_BYTES, "message length {bytes_len} exceeds buffer capacity {CAP_BYTES}");

		// reserve space by draining as much as possible and moving the rest forward
		if CAP_BYTES - self.bytes.write_idx < bytes_len || CAP_FDS - self.fds.write_idx < fds_len {
			match self.poll_flush() {
				Poll::Ready(Ok(())) | Poll::Pending => (),
				Poll::Ready(Err(err)) => return Err(err),
			}
			// move bytes towards front of buffer, maintaining word alignment
			let byte_start = self.bytes.read_idx;
			let byte_end = self.bytes.write_idx;
			let word_start = byte_start / WORD_SIZE; // round down in case a partial word was sent
			let word_end = div_exact(byte_end, "write_idx");
			self.bytes.buf.copy_within(word_start..word_end, 0);
			self.bytes.read_idx -= word_start * WORD_SIZE;
			self.bytes.write_idx -= word_start * WORD_SIZE;
			trace!("copied bytes {}..{} to {}..{}", byte_start, byte_end, self.bytes.read_idx, self.bytes.write_idx);

			// move fds to front of buffer, no alignment concerns
			let (fds_start, fds_end) = (self.fds.read_idx, self.fds.write_idx);
			self.fds.buf.copy_within(fds_start..fds_end, 0);
			self.fds.read_idx = 0;
			self.fds.write_idx = fds_end - fds_start;
			trace!("copied fds {fds_start}..{fds_end} to {}..{}", self.fds.read_idx, self.fds.write_idx);
		}
		if CAP_BYTES - self.bytes.write_idx < bytes_len {
			// still no room
			return Err(Error::new(ErrorKind::Other, format!("failed to reserve {bytes_len} bytes in buffer")));
		}
		if CAP_FDS - self.fds.write_idx < fds_len {
			return Err(Error::new(
				ErrorKind::Other,
				format!("failed to reserve {fds_len} file descriptors in buffer"),
			));
		}

		let write_start = div_exact(self.bytes.write_idx, "write_idx");
		self.bytes.buf[write_start] = object_id.into();
		self.bytes.buf[write_start + 1] = ((bytes_len as u32) << 16) | opcode as u32;
		let write_start = write_start + 2;
		let fd_start = self.fds.write_idx;
		Ok(SendMessage {
			bytes: &mut *self.bytes,
			words_idx: write_start,
			words_goal: write_start + args_len,
			fds: &mut *self.fds,
			fds_idx: fd_start,
			fds_goal: fd_start + fds_len,
		})
	}

	/// Send as much data as possible to the connected peer until sending would block or fail.
	pub fn poll_flush(&mut self) -> Poll<Result<()>> {
		while self.bytes.read_idx < self.bytes.write_idx || self.fds.read_idx < self.fds.write_idx {
			let buf_bytes = Buffer::bytes(&self.bytes.buf);
			let bytes = &buf_bytes[self.bytes.read_idx..self.bytes.write_idx];
			let fds = ControlMessage::ScmRights(&self.fds.buf[self.fds.read_idx..self.fds.write_idx]);
			let n = ready!(cvt_poll(sendmsg(
				self.sock.as_raw_fd(),
				&[IoSlice::new(bytes)],
				&[fds],
				MsgFlags::empty(),
				None::<&()>
			)))?;
			self.bytes.read_idx += n;
			// XXX can sendmsg send partial ancillary data, and how is that reported?
			self.fds.read_idx = self.fds.write_idx;
		}
		Poll::Ready(Ok(()))
	}
}

#[derive(Debug)]
pub struct SendMessage<'c> {
	/// Buffer of bytes to be sent.
	bytes: &'c mut Buffer,
	/// Current write cursor into `bytes.buf`, in *words*.
	words_idx: usize,
	/// Final write cursor into `bytes.buf`, in *words*.
	words_goal: usize,
	/// Buffer of file descriptors to be sent.
	fds: &'c mut FdBuffer,
	/// Current write cursor into `fds.buf`.
	fds_idx: usize,
	/// Final write cursor into `fds.buf`.
	fds_goal: usize,
}

impl<'c> SendMessage<'c> {
	pub fn write(&mut self, word: Word) {
		self.write_all(&[word])
	}

	pub fn write_all(&mut self, words: &[Word]) {
		assert!(self.words_idx + words.len() <= self.words_goal, "message overran requested byte buffers");
		self.bytes.buf[self.words_idx..self.words_idx + words.len()].copy_from_slice(words);
		self.words_idx += words.len();
	}

	pub fn write_fd(&mut self, fd: &impl AsRawFd) {
		assert!(self.fds_idx < self.fds_goal, "message overran requested fd buffers");
		self.fds.buf[self.fds_idx] = fd.as_raw_fd();
		self.fds_idx += 1;
	}

	pub fn finish(self) {
		assert!(self.words_idx == self.words_goal, "message underran requested byte buffers");
		assert!(self.fds_idx == self.fds_goal, "message underran requested fd buffers");
		self.bytes.write_idx = self.words_goal * WORD_SIZE;
		self.fds.write_idx = self.fds_goal * WORD_SIZE;
	}
}
