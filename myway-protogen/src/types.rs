use std::num::NonZeroU32;

/// A Wayland protocol extension, or the core protocol itself.
#[derive(Clone, Debug)]
pub struct Protocol<'doc> {
	pub name: &'doc str,
	pub copyright: Option<&'doc str>,
	pub desc: Option<Description<'doc>>,
	pub interfaces: Vec<Interface<'doc>>,
}

#[derive(Clone, Debug)]
pub struct Interface<'doc> {
	pub name: &'doc str,
	pub version: NonZeroU32,
	pub desc: Option<Description<'doc>>,
}

#[derive(Copy, Clone, Debug)]
pub struct Description<'doc> {
	pub summary: &'doc str,
	pub description: &'doc str,
}
