use std::os::unix::prelude::OwnedFd;

mod args;
mod event;
mod fixed;
mod id;

pub use self::{args::DecodeArg, event::EncodeArg, fixed::Fixed, id::Id};

/// A single protocol word. Messages are always a multiple of this size.
pub type Word = u32;

/// Size of a [`Word`], in bytes.
pub const WORD_SIZE: usize = std::mem::size_of::<Word>();

/// An owned file descriptor, passed over the socket for shared memory or bulk data transfer.
pub type Fd = OwnedFd;

#[allow(unused_imports, dead_code, clippy::enum_variant_names)]
mod generated {
	include!(concat!(env!("OUT_DIR"), "/wayland_protocol.rs"));
}

pub use generated::*;
