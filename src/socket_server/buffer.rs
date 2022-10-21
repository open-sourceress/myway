use std::{
	fmt::{self, Debug, Formatter},
	num::Wrapping,
};

/// Ring buffer of data and bidirectional transformer from bytes to Wayland protocol words (`u32`).
///
/// This type is suitable for buffering bytes recieved from the client into Wayland requests or Wayland events/errors
/// from the server into bytes to send to the client.
#[derive(Clone)]
pub(super) struct Buffer {
	/// Heap-allocated buffer, initialized for convenience.
	buffer: Box<[u32; Self::CAPACITY_WORDS]>,
	/// **byte** index marking start of logically filled data ready to be consumed.
	copyout_idx: Wrapping<usize>,
	/// **byte** index marking end of logically filled data and start of unfilled space for queueing.
	copyin_idx: Wrapping<usize>,
}

impl Buffer {
	/// Buffer capacity, in bytes.
	const CAPACITY_BYTES: usize = 4096;
	/// Buffer capacity, in words.
	const CAPACITY_WORDS: usize = Self::CAPACITY_BYTES / std::mem::size_of::<u32>();

	/// Create a new, empty buffer.
	pub(super) fn new() -> Self {
		Self { buffer: Box::new([0; Self::CAPACITY_WORDS]), copyout_idx: Wrapping(0), copyin_idx: Wrapping(0) }
	}

	fn buffer_bytes(&self) -> &[u8; Self::CAPACITY_BYTES] {
		use std::mem::{align_of, size_of};
		assert_eq!(size_of::<[u32; Self::CAPACITY_WORDS]>(), size_of::<[u8; Self::CAPACITY_BYTES]>());
		assert!(align_of::<[u32; Self::CAPACITY_WORDS]>() >= align_of::<[u8; Self::CAPACITY_BYTES]>());
		unsafe {
			let ptr: *const [u32; Self::CAPACITY_WORDS] = &*self.buffer;
			&*(ptr as *const [u8; Self::CAPACITY_BYTES])
		}
	}

	fn buffer_bytes_mut(&mut self) -> &mut [u8; Self::CAPACITY_BYTES] {
		use std::mem::{align_of, size_of};
		assert_eq!(size_of::<[u32; Self::CAPACITY_WORDS]>(), size_of::<[u8; Self::CAPACITY_BYTES]>());
		assert!(align_of::<[u32; Self::CAPACITY_WORDS]>() >= align_of::<[u8; Self::CAPACITY_BYTES]>());
		unsafe {
			let ptr: *mut [u32; Self::CAPACITY_WORDS] = &mut *self.buffer;
			&mut *(ptr as *mut [u8; Self::CAPACITY_BYTES])
		}
	}

	pub(super) fn byte_data(&self) -> &[u8] {
		let buf = self.buffer_bytes();
		let (Wrapping(copyout_idx), Wrapping(copyin_idx)) = (self.copyout_idx, self.copyin_idx);
		&buf[copyout_idx..copyin_idx]
	}

	pub(super) fn mark_bytes_consumed(&mut self, len: usize) {
		assert!(self.copyout_idx.0 + len <= self.copyin_idx.0);
		self.copyout_idx += len;
		if self.copyout_idx == self.copyin_idx {
			self.copyout_idx = Wrapping(0);
			self.copyin_idx = Wrapping(0);
		}
	}

	pub(super) fn byte_space_mut(&mut self) -> &mut [u8] {
		let Wrapping(copyin_idx) = self.copyin_idx;
		let buf = self.buffer_bytes_mut();
		&mut buf[copyin_idx..]
	}

	pub(super) fn mark_bytes_filled(&mut self, len: usize) {
		self.copyin_idx += len;
	}

	pub(super) fn read_message(&mut self) -> Option<(u32, u16, &[u32])> {
		let (Wrapping(copyout_idx), Wrapping(copyin_idx)) = (self.copyout_idx, self.copyin_idx);
		assert!(copyout_idx % 4 == 0, "copyout_idx ({copyout_idx}) is not word-aligned");
		assert!(copyin_idx % 4 == 0, "copyin_idx ({copyin_idx}) is not word-aligned");
		let (copyout_idx, copyin_idx) = (copyout_idx / 4, copyin_idx / 4);
		let (&obj_id, &len_op, rest) = match &self.buffer[copyout_idx..copyin_idx] {
			[a, b, rest @ ..] => (a, b, rest),
			_ => return None,
		};
		let (byte_len, op) = {
			let [hi1, hi2, lo1, lo2] = len_op.to_be_bytes();
			(u16::from_be_bytes([hi1, hi2]), u16::from_be_bytes([lo1, lo2]))
		};
		if byte_len < 8 || byte_len % 4 != 0 {
			todo!("reject client with invalid len");
		}
		let word_len = usize::from(byte_len / 4) - 2;
		if rest.len() < word_len {
			return None;
		}
		self.copyout_idx += 4 * (2 + word_len);
		Some((obj_id, op, &rest[..word_len]))
	}
}

impl Debug for Buffer {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.debug_struct("Buffer")
			.field("capacity_bytes", &Self::CAPACITY_BYTES)
			.field("copyout_idx", &self.copyout_idx)
			.field("copyin_idx", &self.copyin_idx)
			.finish()
	}
}
