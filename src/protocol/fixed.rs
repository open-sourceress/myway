use crate::client::{RecvMessage, SendMessage};

use super::{DecodeArg, EncodeArg};
use std::io::Result;

/// A signed fixed-point rational number with sign bit, 23 bit integer precision, and 8 bit fractional precision.
#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Fixed(i32);

impl<'a> DecodeArg<'a> for Fixed {
	fn decode_arg(message: &mut RecvMessage<'a>) -> Result<Self> {
		i32::decode_arg(message).map(Fixed)
	}
}

impl EncodeArg for Fixed {
	fn encoded_len(&self) -> u16 {
		self.0.encoded_len()
	}

	fn encode(&self, event: &mut SendMessage<'_>) {
		self.0.encode(event)
	}
}
