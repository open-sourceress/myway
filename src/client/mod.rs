use crate::{
	object_impls::Display,
	object_map::Objects,
	protocol::{Id, Word, WORD_SIZE},
};
use nix::cmsg_space;
use std::{
	fmt, mem,
	os::unix::{io::RawFd, net::UnixStream},
};

pub use self::{
	recv::{RecvHalf, RecvMessage},
	send::{SendHalf, SendMessage},
};

mod recv;
mod send;

/// Capacity of the buffer on each half of the socket, in bytes.
const CAP_BYTES: usize = 4096;
/// Capacity of the buffer on each half of the socket, in words.
const CAP_WORDS: usize = CAP_BYTES / WORD_SIZE;
/// Capacity of the file descriptor buffer on each half of the socket.
const CAP_FDS: usize = 8;

#[allow(clippy::assertions_on_constants)] // that's the point
const _: () = {
	assert!(CAP_BYTES.is_power_of_two(), "buffer capacity is not a power of 2");
	assert!(CAP_WORDS.is_power_of_two(), "buffer capacity is not a power of 2");
	assert!(CAP_BYTES % WORD_SIZE == 0, "buffer capacity is not a multiple of the word size");
};

/// Assert that a byte index into a [`Buffer`] is on a word boundary, and convert it into a word index.
///
/// If the index is in the middle of a word (in other words, not a multiple of `WORD_SIZE`), this call panics with a
/// message including `what` the index was (which should be `"read_idx"` or `"write_idx"`).
#[track_caller]
fn div_exact(n: usize, what: &'static str) -> usize {
	assert!(n % WORD_SIZE == 0, "{what} {n} is not aligned to a word boundary ({WORD_SIZE})");
	n / WORD_SIZE
}

#[derive(Debug)]
pub struct Client {
	/// Socket used to communicate with the client
	sock: UnixStream,
	/// Outgoing message bytes
	tx_bytes: Buffer,
	/// Outgoing file descriptors
	tx_fds: FdBuffer,
	/// Incoming message bytes
	rx_bytes: Buffer,
	/// Incoming file descriptors
	rx_fds: FdBuffer,
	/// Preallocated space for recvmsg(2) control messages
	rx_cmsg: Vec<u8>,
	/// Objects allocated to this client
	objects: Objects,
}

impl Client {
	/// Create client state wrapping the peer connected to the provided socket.
	pub fn new(sock: UnixStream) -> Self {
		let mut objects = Objects::new();
		objects.insert(Id::<Display>::new(1).unwrap(), Display).unwrap();
		Self {
			sock,
			tx_bytes: Buffer::new(),
			tx_fds: FdBuffer::new(),
			rx_bytes: Buffer::new(),
			rx_fds: FdBuffer::new(),
			rx_cmsg: cmsg_space!([RawFd; CAP_FDS]),
			objects,
		}
	}

	/// Split this client state into handles for its constituent parts.
	///
	/// The three returned values are:
	///
	/// - [`SendHalf`], for sending events to the connected client
	/// - [`RecvHalf`], for polling requests from the client
	/// - [`Objects`], tracking object IDs allocated to this client
	///
	/// Splitting with this method allows minimizing copies of protocol data: requests are read into the receiver's
	/// buffers, request args are parsed directly from that buffer, and response events are written into space reserved
	/// in the sender's buffers.
	pub fn split_mut(&mut self) -> (send::SendHalf<'_>, recv::RecvHalf<'_>, &mut Objects) {
		(
			send::SendHalf { sock: &self.sock, bytes: &mut self.tx_bytes, fds: &mut self.tx_fds },
			recv::RecvHalf {
				sock: &self.sock,
				bytes: &mut self.rx_bytes,
				fds: &mut self.rx_fds,
				cmsg_buf: &mut self.rx_cmsg,
			},
			&mut self.objects,
		)
	}
}

/// Buffer of incoming or outgoing message data, accessible as bytes or words.
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

	// can be const with #![feature(const_mut_ref)] <https://github.com/rust-lang/rust/issues/57349>
	#[allow(clippy::needless_lifetimes)] // for explicitness around unsae
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

/// Buffer of incoming or outgoing file descriptors.
struct FdBuffer {
	buf: Box<[RawFd; CAP_FDS]>,
	read_idx: usize,
	write_idx: usize,
}

impl FdBuffer {
	fn new() -> Self {
		Self { buf: Box::new([-1; CAP_FDS]), read_idx: 0, write_idx: 0 }
	}
}

impl fmt::Debug for FdBuffer {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("FdBuffer")
			.field("capacity", &CAP_FDS)
			.field("read_idx", &self.read_idx)
			.field("write_idx", &self.write_idx)
			.finish()
	}
}
