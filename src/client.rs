use crate::{cvt_poll, object_impls::Display};
use log::trace;
use std::{
	fmt,
	io::{Error, ErrorKind, Read, Result, Write},
	mem,
	os::unix::{io::AsRawFd, net::UnixStream},
	task::{ready, Poll},
};

/// A Wayland protocol word, the smallest unit of a message.
type Word = u32;
/// Size of a [`Word`], extracted into a const for convenience.
const WORD_SIZE: usize = mem::size_of::<Word>();
/// Capacity of the buffer on each half of the socket, in bytes.
const CAP_BYTES: usize = 4096;
/// Capacity of the buffer on each half of the socket, in words.
const CAP_WORDS: usize = CAP_BYTES / WORD_SIZE;

#[allow(clippy::assertions_on_constants)] // that's the point
const _: () = {
	assert!(CAP_BYTES.is_power_of_two(), "buffer capacity is not a power of 2");
	assert!(WORD_SIZE.is_power_of_two(), "buffer capacity is not a power of 2");
	assert!(CAP_BYTES % WORD_SIZE == 0, "buffer capacity is not a multiple of the word size");
};

#[track_caller]
fn div_exact(n: usize, what: &'static str) -> usize {
	assert!(n % WORD_SIZE == 0, "{what} {n} is not aligned to a word boundary ({WORD_SIZE})");
	n / WORD_SIZE
}

#[derive(Debug)]
pub struct Client {
	/// Socket used to communicate with the client
	sock: UnixStream,
	/// Buffered messages to be sent
	tx: Buffer,
	/// Buffered messages to be processed
	rx: Buffer,
	display: Option<Display>,
}

impl Client {
	pub fn new(sock: UnixStream) -> Self {
		Self { sock, tx: Buffer::new(), rx: Buffer::new(), display: Some(Display) }
	}

	pub fn split_mut(&mut self) -> (SendHalf<'_>, RecvHalf<'_>, &mut Option<Display>) {
		(
			SendHalf { sock: &self.sock, buf: &mut self.tx },
			RecvHalf { sock: &self.sock, buf: &mut self.rx },
			&mut self.display,
		)
	}
}

#[derive(Debug)]
pub struct SendHalf<'c> {
	sock: &'c UnixStream,
	buf: &'c mut Buffer,
}

impl<'c> SendHalf<'c> {
	/// Submit an event or error to this client.
	///
	/// Submission is atomic: either the message is enqueued in full and the method returns `Ok`, or the message was
	/// not queued and this method returns `Err`. The message is never partially enqueued.
	///
	/// This method appends the message content to an internal buffer, flushing bytes from that buffer to the client
	/// only if necessary to fit the provided message. To ensure messages are delivered in a timely manner, call
	/// [`poll_flush`](Self::poll_flush) after this method.
	pub fn submit(&mut self, message: &[Word]) -> Result<()> {
		let byte_len = message.len() * WORD_SIZE;
		trace!("submitting message of {byte_len} bytes");
		assert!(byte_len <= CAP_BYTES, "cannot write {byte_len} bytes into a buffer of {CAP_BYTES} bytes");

		// if there isn't enough space, try flushing some buffered bytes to make room
		if CAP_BYTES - self.buf.write_idx < byte_len {
			trace!("no room for {byte_len}-byte message, trying to make space");
			match self.poll_flush() {
				Poll::Ready(Ok(())) => (),
				Poll::Ready(Err(err)) => return Err(err),
				Poll::Pending => {
					// Some bytes are left in the buffer. Move them to the front to try to make room. Only move whole
					// words to ensure write_idx is always word-aligned.
					let buf = &mut self.buf;
					let start = buf.read_idx / WORD_SIZE; // intentional truncation
					let end = div_exact(buf.write_idx, "write_idx");
					trace!("moving unflushed data at {}..{} forward by {} words", buf.read_idx, buf.write_idx, start);
					buf.buf.copy_within(start..end, 0);
					buf.read_idx -= start * WORD_SIZE;
					buf.write_idx -= start * WORD_SIZE;
					trace!("data moved to {}..{}", buf.read_idx, buf.write_idx);
				},
			}
		}
		// if there's still no room, assume the client is running very slow and don't wait for them to catch up
		if CAP_BYTES - self.buf.write_idx < byte_len {
			return Err(Error::new(ErrorKind::Other, "unable to reserve buffer space for a message"));
		}
		let buf = &mut self.buf;
		let start = div_exact(buf.write_idx, "write_idx");
		buf.buf[start..start + message.len()].copy_from_slice(message);
		trace!("wrote message to buffer, bytes {}..{}", start * WORD_SIZE, (start + message.len()) * WORD_SIZE);
		buf.write_idx += byte_len;
		Ok(())
	}

	/// Flush buffered messages, delivering them to the client. Returns `Ready(Ok(()))` if the buffer was flushed
	/// completely, or `Pending` if there are still messages to deliver.
	pub fn poll_flush(&mut self) -> Poll<Result<()>> {
		let buf = &mut self.buf;
		let bytes = Buffer::bytes(&buf.buf);
		while buf.read_idx < buf.write_idx {
			let data = &bytes[buf.read_idx..buf.write_idx];
			trace!("> write(fd={}, buf=[len={}])", self.sock.as_raw_fd(), data.len());
			let n = ready!(cvt_poll(self.sock.write(data)))?;
			trace!("< {n}");
			if n == 0 {
				return Poll::Ready(Err(ErrorKind::WriteZero.into()));
			}
			buf.read_idx += n;
		}
		cvt_poll(self.sock.flush())
	}
}

#[derive(Debug)]
pub struct RecvHalf<'c> {
	sock: &'c UnixStream,
	buf: &'c mut Buffer,
}

impl<'c> RecvHalf<'c> {
	/// Receive a request from this client, if one is ready.
	///
	/// Returns `(object_id, opcode, arg_words)`. Parsing `arg_words` into request arguments is left to the caller.
	pub fn poll_recv(&mut self) -> Poll<Result<(u32, u16, &[Word])>> {
		// read header: [(0:31 object_id), (0:15 opcode, 16:31 message length)]
		let msg_len = match ready!(self.fill_words(2, false))? {
			&[_id, len_op] => (len_op >> 16) as usize,
			_ => unreachable!(),
		};
		if msg_len < 2 * WORD_SIZE || msg_len % WORD_SIZE != 0 {
			todo!("reject client on protocol error");
		}
		// with the validated message length we can now decode the complete packet
		let (obj_id, opcode, args) = match ready!(self.fill_words(msg_len / WORD_SIZE, true))? {
			&[id, len_op, ref args @ ..] => (id, len_op as u16, args),
			_ => unreachable!(),
		};
		Poll::Ready(Ok((obj_id, opcode, args)))
	}

	/// Ensure `rx` contains at least `len` *words*, and return them.
	///
	/// Iff `consume` is true and `word_len` words are successfully read into the buffer, `read_idx` is updated to point
	/// past the returned words, effectively removing them from the buffer.
	fn fill_words(&mut self, word_len: usize, consume: bool) -> Poll<Result<&[Word]>> {
		let byte_len = word_len * WORD_SIZE;
		assert!(byte_len < CAP_BYTES, "cannot read {byte_len} bytes into a buffer of {CAP_BYTES} bytes");
		let buf = &mut self.buf;
		let bytes = Buffer::bytes_mut(&mut buf.buf);
		while buf.write_idx - buf.read_idx < byte_len {
			let space = &mut bytes[buf.write_idx..];
			trace!("> read(fd={}, buf=[len={}])", self.sock.as_raw_fd(), space.len());
			let n = ready!(cvt_poll(self.sock.read(space)))?;
			trace!("< {n}");
			if n == 0 {
				return Poll::Ready(Err(ErrorKind::UnexpectedEof.into()));
			}
			buf.write_idx += n;
		}
		let start = div_exact(buf.read_idx, "read_idx");
		let end = buf.write_idx / WORD_SIZE; // allow this to truncate to ignore a partially-read word at the end
		assert!(end - start >= word_len, "fill_words: the range {start}..{end} does not contain {word_len} words");
		if consume {
			buf.read_idx += word_len * WORD_SIZE;
		}
		Poll::Ready(Ok(&buf.buf[start..start + word_len]))
	}
}

struct Buffer {
	/// Internal buffer of *bytes*, typed as `[Word]` to ensure alignment
	buf: Box<[Word; CAP_WORDS]>,
	/// *Byte* index of logically filled data to be consumed
	read_idx: usize,
	/// *Byte* index of logically unfilled space to be filled
	write_idx: usize,
}

impl Buffer {
	fn new() -> Self {
		Self { buf: Box::new([0; CAP_WORDS]), read_idx: 0, write_idx: 0 }
	}

	#[allow(clippy::needless_lifetimes)] // for explicitness around unsafe
	const fn bytes<'b>(words: &'b [Word; CAP_WORDS]) -> &'b [u8; CAP_BYTES] {
		assert!(mem::size_of::<[Word; CAP_WORDS]>() == mem::size_of::<[u8; CAP_BYTES]>());
		assert!(mem::align_of::<[Word; CAP_WORDS]>() >= mem::align_of::<[u8; CAP_BYTES]>());
		// Safety:
		// - &T ensures the input is not null, and the output copies its address, so the output is not null.
		// - &T ensures the input is aligned for the source type, and we asserted that this makes it properly aligned
		//   for the target type.
		// - &T ensures the input is dereferenceable for the size of the source type, and we asserted that this makes it
		//   dereferenceable for the size of the target type.
		// - &T ensures the input is initialized, and so the output is initialized. The source type has no padding or
		//   uninitialized bytes.
		// - Every instance of the source type is valid at the output type because the output type has no invalid bit
		//   patterns.
		// - The lifetime of the output is tied to the lifetime of the input, ensuring Rust's aliasing rules are upheld.
		// Endianness: Wayland uses native byte order, so the implicit `to_ne_bytes` in this cast is correct.
		unsafe { &*(words as *const [Word; CAP_WORDS] as *const [u8; CAP_BYTES]) }
	}

	#[allow(clippy::needless_lifetimes)] // for explicitness around unsae
									 // can be const with #![feature(const_mut_ref)] <https://github.com/rust-lang/rust/issues/57349>
	fn bytes_mut<'b>(words: &'b mut [Word; CAP_WORDS]) -> &'b mut [u8; CAP_BYTES] {
		assert!(mem::size_of::<[Word; CAP_WORDS]>() == mem::size_of::<[u8; CAP_BYTES]>());
		assert!(mem::align_of::<[Word; CAP_WORDS]>() >= mem::align_of::<[u8; CAP_BYTES]>());
		// Safety: see Self::bytes
		unsafe { &mut *(words as *mut [Word; CAP_WORDS] as *mut [u8; CAP_BYTES]) }
	}
}

impl fmt::Debug for Buffer {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("Buffer")
			.field("capacity", &CAP_BYTES)
			.field("read_idx", &self.read_idx)
			.field("write_idx", &self.write_idx)
			.finish()
	}
}
