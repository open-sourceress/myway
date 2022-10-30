use super::{Fd, Word, WORD_SIZE};
use log::trace;
use std::io::{Error, ErrorKind, Result};

#[derive(Debug)]
pub struct Args<'a> {
	words: &'a [Word],
}

impl<'a> Args<'a> {
	pub fn new(words: &'a [Word]) -> Self {
		Self { words }
	}

	pub fn take(&mut self) -> Result<Word> {
		match self.words {
			&[x, ref rest @ ..] => {
				self.words = rest;
				Ok(x)
			},
			[] => Err(Error::new(ErrorKind::InvalidInput, "too few arguments")),
		}
	}

	pub fn take_n(&mut self, n: usize) -> Result<&'a [Word]> {
		if n <= self.words.len() {
			let (arg, rest) = self.words.split_at(n);
			self.words = rest;
			Ok(arg)
		} else {
			Err(Error::new(ErrorKind::InvalidInput, "too few arguments"))
		}
	}

	pub fn take_fd(&mut self) -> Result<Fd> {
		todo!("file descriptors")
	}

	pub fn finish(self) -> Result<()> {
		if self.words.is_empty() {
			Ok(())
		} else {
			Err(Error::new(ErrorKind::InvalidInput, "too many arguments"))
		}
	}
}

pub trait FromArgs<'a>: Sized {
	fn from_args(args: &mut Args<'a>) -> Result<Self>;
}

impl<'a> FromArgs<'a> for u32 {
	fn from_args(args: &mut Args<'a>) -> Result<Self> {
		args.take()
	}
}

impl<'a> FromArgs<'a> for i32 {
	fn from_args(args: &mut Args<'a>) -> Result<Self> {
		Ok(args.take()? as i32)
	}
}

impl<'a> FromArgs<'a> for &'a str {
	fn from_args(args: &mut Args<'a>) -> Result<Self> {
		let byte_len = u32::from_args(args)?;
		match byte_len {
			0 => Err(Error::new(ErrorKind::InvalidInput, "string argument must not be null")),
			n => split_string_common(n, args),
		}
	}
}

impl<'a> FromArgs<'a> for Option<&'a str> {
	fn from_args(args: &mut Args<'a>) -> Result<Self> {
		let byte_len = u32::from_args(args)?;
		match byte_len {
			0 => Ok(None),
			n => split_string_common(n, args).map(Some),
		}
	}
}

fn split_string_common<'a>(byte_len: u32, args: &mut Args<'a>) -> Result<&'a str> {
	let word_len = (byte_len as usize + WORD_SIZE - 1) / WORD_SIZE; // divide by word size, rounded up
	trace!("taking {word_len} words ({byte_len} bytes)");
	let arg_words = args.take_n(word_len)?;
	// Safety: casting [Word; N] to equivalent [u8; N*WORD_SIZE]
	// strings are transferred native-endian so the implicit to_ne_bytes is correct
	let arg_bytes: &'a [u8] =
		unsafe { std::slice::from_raw_parts(arg_words.as_ptr().cast(), arg_words.len() * WORD_SIZE) };
	let bytes = match arg_bytes[..byte_len as usize] {
		[ref s @ .., 0] => s,
		_ => return Err(Error::new(ErrorKind::InvalidInput, "string argument not NUL-terminated")),
	};
	if bytes.iter().any(|&b| b == 0) {
		return Err(Error::new(ErrorKind::InvalidInput, "string argument has interior NULs"));
	}
	let string = std::str::from_utf8(bytes).map_err(|err| Error::new(ErrorKind::InvalidInput, err))?;
	Ok(string)
}

impl<'a> FromArgs<'a> for &'a [Word] {
	fn from_args(args: &mut Args<'a>) -> Result<Self> {
		let word_len = u32::from_args(args)?;
		args.take_n(word_len as usize)
	}
}

impl<'a> FromArgs<'a> for Fd {
	fn from_args(args: &mut Args<'a>) -> Result<Self> {
		args.take_fd()
	}
}
