use super::{Buffer, FdBuffer, CAP_BYTES, CAP_FDS};
use crate::{
	cvt_poll,
	protocol::{AnyObject, Id, Word, WORD_SIZE},
};
use log::trace;
use nix::sys::socket::{recvmsg, ControlMessageOwned, MsgFlags};
use std::{
	io::{Error, ErrorKind, IoSliceMut, Result},
	os::unix::{
		io::{FromRawFd, OwnedFd},
		net::UnixStream,
		prelude::AsRawFd,
	},
	task::{ready, Poll},
};

#[derive(Debug)]
pub struct RecvHalf<'c> {
	pub(super) sock: &'c UnixStream,
	pub(super) bytes: &'c mut Buffer,
	pub(super) fds: &'c mut FdBuffer,
	pub(super) cmsg_buf: &'c mut Vec<u8>,
}

impl<'c> RecvHalf<'c> {
	pub fn poll_recv(&mut self) -> Poll<Result<RecvMessage<'_>>> {
		let byte_len = match ready!(fill_words(self.sock, self.bytes, self.fds, self.cmsg_buf, 2, false))? {
			&[_obj, len_op] => len_op as usize >> 16,
			_ => unreachable!(),
		};
		if byte_len < 8 {
			return Poll::Ready(Err(Error::new(
				ErrorKind::InvalidInput,
				"message length must be larger than message header",
			)));
		}
		if byte_len % WORD_SIZE != 0 {
			return Poll::Ready(Err(Error::new(
				ErrorKind::InvalidInput,
				"message length must be a multiple of the word size",
			)));
		}
		let (object_id, opcode, args) =
			match ready!(fill_words(self.sock, self.bytes, self.fds, self.cmsg_buf, byte_len / WORD_SIZE, true))? {
				&[obj, len_op, ref args @ ..] => (obj, len_op as u16, args),
				_ => unreachable!(),
			};
		let object_id =
			Id::new(object_id).ok_or_else(|| Error::new(ErrorKind::InvalidInput, "message target cannot be null"))?;
		Poll::Ready(Ok(RecvMessage { object_id, opcode, bytes: args, fds: self.fds }))
	}
}

/// Ensure `buf` contains at least `word_len` *words*, and return them.
///
/// Iff `consume` is true and `word_len` words are successfully read into the buffer, `read_idx` is updated to point
/// past the returned words, effectively removing them from the buffer.
fn fill_words<'b>(
	sock: &UnixStream,
	buf: &'b mut Buffer,
	fds: &mut FdBuffer,
	cmsg_buf: &'b mut Vec<u8>,
	word_len: usize,
	consume: bool,
) -> Poll<Result<&'b [Word]>> {
	let byte_len = word_len * WORD_SIZE;
	assert!(byte_len < CAP_BYTES, "cannot read {byte_len} bytes into a buffer of {CAP_BYTES} bytes");
	let bytes = Buffer::bytes_mut(&mut buf.buf);
	while buf.write_idx - buf.read_idx < byte_len {
		let space = &mut bytes[buf.write_idx..];

		trace!(
			"> recvmsg(sockfd={}, iov[0]=[len={}], control[0]=[len={}], flags={:?})",
			sock.as_raw_fd(),
			space.len(),
			cmsg_buf.len(),
			MsgFlags::MSG_CMSG_CLOEXEC
		);
		let msg = ready!(cvt_poll(recvmsg::<()>(
			sock.as_raw_fd(),
			&mut [IoSliceMut::new(space)],
			Some(cmsg_buf),
			MsgFlags::MSG_CMSG_CLOEXEC
		)))?;
		trace!("< bytes={}, flags={:?}", msg.bytes, msg.flags);
		if msg.flags.contains(MsgFlags::MSG_CTRUNC) {
			todo!("shut down connection, file descriptor discarded");
		}
		for msg in msg.cmsgs() {
			if let ControlMessageOwned::ScmRights(msg_fds) = msg {
				let n = Ord::min(msg_fds.len(), CAP_FDS - fds.write_idx);
				fds.buf[fds.write_idx..fds.write_idx + n].copy_from_slice(&msg_fds[..n]);
				if n < msg_fds.len() {
					todo!("too many file descriptors");
				}
			}
		}

		if msg.bytes == 0 {
			return Poll::Ready(Err(ErrorKind::UnexpectedEof.into()));
		}
		buf.write_idx += msg.bytes;
	}
	let start = super::div_exact(buf.read_idx, "read_idx");
	let end = buf.write_idx / WORD_SIZE; // allow this to truncate to ignore a partially-read word at the end
	assert!(end - start >= word_len, "fill_words: the range {start}..{end} does not contain {word_len} words");
	if consume {
		buf.read_idx += word_len * WORD_SIZE;
	}
	Poll::Ready(Ok(&buf.buf[start..start + word_len]))
}

#[derive(Debug)]
pub struct RecvMessage<'c> {
	object_id: Id<AnyObject>,
	opcode: u16,
	bytes: &'c [Word],
	fds: &'c mut FdBuffer,
}

impl<'c> RecvMessage<'c> {
	pub fn object_id(&self) -> Id<AnyObject> {
		self.object_id
	}

	pub fn opcode(&self) -> u16 {
		self.opcode
	}

	pub fn take(&mut self) -> Result<u32> {
		match *self.bytes {
			[arg, ref rest @ ..] => {
				self.bytes = rest;
				Ok(arg)
			},
			[] => Err(Error::new(ErrorKind::InvalidInput, "too few args")),
		}
	}

	pub fn split(&mut self, n: usize) -> Result<&'c [u32]> {
		if self.bytes.len() < n {
			return Err(Error::new(ErrorKind::InvalidInput, "too few args"));
		}
		let (arg, rest) = self.bytes.split_at(n);
		self.bytes = rest;
		Ok(arg)
	}

	pub fn take_fd(&mut self) -> Result<OwnedFd> {
		if self.fds.read_idx < self.fds.write_idx {
			return Err(Error::new(ErrorKind::InvalidInput, "too few file descriptors"));
		}
		let fd = self.fds.buf[self.fds.read_idx];
		self.fds.read_idx += 1;
		// Safety: kernel ensures that file descriptors from recvmsg() are valid opened file descriptors, and
		// incrementing read_idx before returning from this call ensures that file descriptors aren't returned twice
		Ok(unsafe { OwnedFd::from_raw_fd(fd) })
	}

	pub fn finish(self) -> Result<()> {
		if self.bytes.is_empty() {
			Ok(())
		} else {
			Err(Error::new(ErrorKind::InvalidInput, "too many args"))
		}
	}
}
