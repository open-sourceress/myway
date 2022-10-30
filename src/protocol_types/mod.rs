use std::os::unix::prelude::OwnedFd;

mod args;
mod fixed;
mod id;

pub use self::{
	args::{Args, FromArgs},
	fixed::Fixed,
	id::Id,
};

/// A single protocol word. Messages are always a multiple of this size.
pub type Word = u32;

/// Size of a [`Word`], in bytes.
pub const WORD_SIZE: usize = std::mem::size_of::<Word>();

/// An owned file descriptor, passed over the socket for shared memory or bulk data transfer.
pub type Fd = OwnedFd;
