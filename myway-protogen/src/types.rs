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
	pub requests: Vec<Message<'doc>>,
	pub events: Vec<Message<'doc>>,
	pub enums: Vec<Enum<'doc>>,
}

#[derive(Clone, Debug)]
pub struct Message<'doc> {
	pub name: &'doc str,
	pub kind: Option<&'doc str>,
	pub since: Option<NonZeroU32>,
	pub desc: Option<Description<'doc>>,
	pub args: Vec<Arg<'doc>>,
}

#[derive(Clone, Debug)]
pub struct Arg<'doc> {
	pub name: &'doc str,
	pub ty: ArgType<'doc>,
	pub summary: Option<&'doc str>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ArgType<'doc> {
	Int,
	Uint { r#enum: Option<&'doc str> },
	Fixed,
	String { nullable: bool },
	Object { interface: Option<&'doc str>, nullable: bool },
	NewId { interface: Option<&'doc str> },
	Array,
	Fd,
}

#[derive(Clone, Debug)]
pub struct Enum<'doc> {
	pub name: &'doc str,
	pub since: Option<NonZeroU32>,
	pub bitfield: bool,
	pub desc: Option<Description<'doc>>,
	pub entries: Vec<Entry<'doc>>,
}

#[derive(Clone, Debug)]
pub struct Entry<'doc> {
	pub name: &'doc str,
	pub value: u32,
	pub value_is_hex: bool,
	pub summary: Option<&'doc str>,
	pub since: Option<NonZeroU32>,
}

#[derive(Copy, Clone, Debug)]
pub struct Description<'doc> {
	pub summary: &'doc str,
	pub description: &'doc str,
}
