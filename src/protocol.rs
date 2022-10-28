use std::{
	io::{Error, ErrorKind, Result},
	num::NonZeroU32,
	os::unix::prelude::OwnedFd,
};

#[allow(unused_imports, dead_code, clippy::enum_variant_names)]
pub mod wayland {
	include!(concat!(env!("OUT_DIR"), "/wayland_protocol.rs"));
}

/// A signed fixed-point rational number with sign bit, 23 bit integer precision, and 8 bit fractional precision.
#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Fixed(i32);

trait FromArgs<'a>: Sized {
	fn split(args: &'a [u32]) -> Result<(Self, &'a [u32])>;
}

impl<'a> FromArgs<'a> for u32 {
	fn split(args: &'a [u32]) -> Result<(Self, &'a [u32])> {
		match *args {
			[arg, ref rest @ ..] => Ok((arg, rest)),
			[] => Err(Error::new(ErrorKind::InvalidInput, "missing argument")),
		}
	}
}

impl<'a> FromArgs<'a> for i32 {
	fn split(args: &'a [u32]) -> Result<(Self, &'a [u32])> {
		u32::split(args).map(|(arg, rest)| (arg as i32, rest))
	}
}

impl<'a> FromArgs<'a> for NonZeroU32 {
	fn split(args: &'a [u32]) -> Result<(Self, &'a [u32])> {
		match <Option<Self>>::split(args)? {
			(Some(arg), rest) => Ok((arg, rest)),
			(None, _) => Err(Error::new(ErrorKind::InvalidInput, "ID may not be null")),
		}
	}
}

impl<'a> FromArgs<'a> for Option<NonZeroU32> {
	fn split(args: &'a [u32]) -> Result<(Self, &'a [u32])> {
		u32::split(args).map(|(arg, rest)| (NonZeroU32::new(arg), rest))
	}
}

impl<'a> FromArgs<'a> for Fixed {
	fn split(args: &'a [u32]) -> Result<(Self, &'a [u32])> {
		i32::split(args).map(|(arg, rest)| (Fixed(arg), rest))
	}
}

impl<'a> FromArgs<'a> for &'a str {
	fn split(args: &'a [u32]) -> Result<(Self, &'a [u32])> {
		let (byte_len, rest) = u32::split(args)?;
		match byte_len {
			0 => Err(Error::new(ErrorKind::InvalidInput, "string argument must not be null")),
			n => split_string_common(n, rest),
		}
	}
}

impl<'a> FromArgs<'a> for Option<&'a str> {
	fn split(args: &'a [u32]) -> Result<(Self, &'a [u32])> {
		let (byte_len, rest) = u32::split(args)?;
		match byte_len {
			0 => Ok((None, rest)),
			n => split_string_common(n, rest).map(|(s, rest)| (Some(s), rest)),
		}
	}
}

fn split_string_common<'a>(byte_len: u32, args: &'a [u32]) -> Result<(&'a str, &'a [u32])> {
	let word_len = (byte_len + 3) / 4; // divide by word size, rounded up
	if word_len as usize > args.len() {
		return Err(Error::new(ErrorKind::InvalidInput, "string argument truncated"));
	}
	let (arg_words, rest) = args.split_at(word_len as usize);
	// Safety: casting [u32; N] to equivalent [u8; N*4]
	// strings are sent native-endian so the implicit to_ne_bytes is correct
	let arg_bytes: &'a [u8] = unsafe { std::slice::from_raw_parts(arg_words.as_ptr().cast(), arg_words.len() * 4) };
	let bytes = match arg_bytes[..byte_len as usize] {
		[ref s @ .., 0] => s,
		_ => return Err(Error::new(ErrorKind::InvalidInput, "string argument not NUL-terminated")),
	};
	if bytes.iter().any(|&b| b == 0) {
		return Err(Error::new(ErrorKind::InvalidInput, "string argument has interior NULs"));
	}
	let string = std::str::from_utf8(bytes).map_err(|err| Error::new(ErrorKind::InvalidInput, err))?;
	Ok((string, rest))
}

impl<'a> FromArgs<'a> for &'a [u32] {
	fn split(args: &'a [u32]) -> Result<(Self, &'a [u32])> {
		let (word_len, rest) = u32::split(args)?;
		if word_len as usize > rest.len() {
			return Err(Error::new(ErrorKind::InvalidInput, "array argument truncated"));
		}
		Ok(rest.split_at(word_len as usize))
	}
}

impl<'a> FromArgs<'a> for OwnedFd {
	fn split(_: &'a [u32]) -> Result<(Self, &'a [u32])> {
		todo!("support reading file descriptors from messages")
	}
}
