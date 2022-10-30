use super::{Fd, Word, WORD_SIZE};
use std::fmt;

pub struct Event<'a> {
	space: &'a mut [Word],
}

impl<'a> Event<'a> {
	pub fn new(space: &'a mut [Word]) -> Self {
		Self { space }
	}

	pub fn write(&mut self, arg: u32) {
		let (word, rest) = std::mem::take(&mut self.space).split_first_mut().unwrap();
		*word = arg;
		self.space = rest;
	}

	pub fn write_all(&mut self, args: &[u32]) {
		let (space, rest) = std::mem::take(&mut self.space).split_at_mut(args.len());
		space.copy_from_slice(args);
		self.space = rest;
	}

	pub fn finish(self) {
		assert!(self.space.is_empty(), "event did not fill its buffer, {} words remaining", self.space.len());
	}
}

impl fmt::Debug for Event<'_> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("Event").field("capacity", &self.space.len()).finish()
	}
}

pub trait ToEvent {
	/// Length of the encoded form of this value, in words.
	fn encoded_len(&self) -> u16;
	/// Encode into an event.
	///
	/// `event` is guaranteed to have at least `self.encoded_len()` words of space remaining. Implementors may panic
	/// but not cause undefined behavior if this precondition is not upheld.
	fn encode(&self, event: &mut Event<'_>);
}

impl ToEvent for u32 {
	fn encoded_len(&self) -> u16 {
		1
	}

	fn encode(&self, event: &mut Event<'_>) {
		event.write(*self);
	}
}

impl ToEvent for i32 {
	fn encoded_len(&self) -> u16 {
		(*self as u32).encoded_len()
	}

	fn encode(&self, event: &mut Event<'_>) {
		(*self as u32).encode(event)
	}
}

impl<'a> ToEvent for &'a str {
	fn encoded_len(&self) -> u16 {
		assert!(self.len() < u16::MAX as usize, "string is too large to serialize");
		let byte_len = self.len() as u16 + 1; // nul terminator
		let word_len = (byte_len + WORD_SIZE as u16 - 1) / WORD_SIZE as u16;
		word_len + 1 // length
	}

	fn encode(&self, event: &mut Event<'_>) {
		(self.len() as u32 + 1).encode(event);
		let (ptr, len) = (self.as_ptr(), self.len());
		let mut i = 0;
		while i + WORD_SIZE <= len {
			let word = unsafe { std::ptr::read_unaligned(ptr.add(i).cast::<Word>()) };
			event.write(word);
			i += WORD_SIZE;
		}
		match self.as_bytes()[i..] {
			[] => event.write(0),
			[a] => event.write(Word::from_ne_bytes([a, 0, 0, 0])),
			[a, b] => event.write(Word::from_ne_bytes([a, b, 0, 0])),
			[a, b, c] => event.write(Word::from_ne_bytes([a, b, c, 0])),
			_ => unreachable!(),
		}
	}
}

impl<'a> ToEvent for Option<&'a str> {
	fn encoded_len(&self) -> u16 {
		match self {
			Some(s) => s.encoded_len(),
			None => 1,
		}
	}

	fn encode(&self, event: &mut Event<'_>) {
		match self {
			Some(s) => s.encode(event),
			None => event.write(0), // len (empty)
		}
	}
}

impl<'a> ToEvent for &'a [Word] {
	fn encoded_len(&self) -> u16 {
		assert!(self.len() < u16::MAX as usize, "string is too large to serialize");
		self.len() as u16 + 1
	}

	fn encode(&self, event: &mut Event<'_>) {
		(self.len() as u32).encode(event);
		event.write_all(self);
	}
}

impl ToEvent for Fd {
	fn encoded_len(&self) -> u16 {
		0
	}

	fn encode(&self, _event: &mut Event<'_>) {
		todo!("serialize file descriptors")
	}
}
