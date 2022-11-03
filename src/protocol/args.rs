use crate::client::RecvMessage;

use super::{Fd, Word, WORD_SIZE};
use log::trace;
use std::io::{Error, ErrorKind, Result};

pub trait DecodeArg<'a>: Sized {
	fn decode_arg(message: &mut RecvMessage<'a>) -> Result<Self>;
}

impl<'a> DecodeArg<'a> for u32 {
	fn decode_arg(message: &mut RecvMessage<'a>) -> Result<Self> {
		message.take()
	}
}

impl<'a> DecodeArg<'a> for i32 {
	fn decode_arg(message: &mut RecvMessage<'a>) -> Result<Self> {
		Ok(message.take()? as i32)
	}
}

impl<'a> DecodeArg<'a> for &'a str {
	fn decode_arg(message: &mut RecvMessage<'a>) -> Result<Self> {
		let byte_len = u32::decode_arg(message)?;
		match byte_len {
			0 => Err(Error::new(ErrorKind::InvalidInput, "string argument must not be null")),
			n => split_string_common(n, message),
		}
	}
}

impl<'a> DecodeArg<'a> for Option<&'a str> {
	fn decode_arg(message: &mut RecvMessage<'a>) -> Result<Self> {
		let byte_len = u32::decode_arg(message)?;
		match byte_len {
			0 => Ok(None),
			n => split_string_common(n, message).map(Some),
		}
	}
}

fn split_string_common<'a>(byte_len: u32, message: &mut RecvMessage<'a>) -> Result<&'a str> {
	let word_len = (byte_len as usize + WORD_SIZE - 1) / WORD_SIZE; // divide by word size, rounded up
	trace!("taking {word_len} words ({byte_len} bytes)");
	let arg_words = message.split(word_len)?;
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

impl<'a> DecodeArg<'a> for &'a [Word] {
	fn decode_arg(message: &mut RecvMessage<'a>) -> Result<Self> {
		let word_len = u32::decode_arg(message)?;
		message.split(word_len as usize)
	}
}

impl<'a> DecodeArg<'a> for Fd {
	fn decode_arg(message: &mut RecvMessage<'a>) -> Result<Self> {
		message.take_fd()
	}
}
