use super::{Args, FromArgs, ToEvent};
use std::io::Result;

/// A signed fixed-point rational number with sign bit, 23 bit integer precision, and 8 bit fractional precision.
#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct Fixed(i32);

impl<'a> FromArgs<'a> for Fixed {
	fn from_args(args: &mut Args<'a>) -> Result<Self> {
		i32::from_args(args).map(Fixed)
	}
}

impl ToEvent for Fixed {
	fn encoded_len(&self) -> u16 {
		self.0.encoded_len()
	}

	fn encode(&self, event: &mut super::Event<'_>) {
		self.0.encode(event)
	}
}
