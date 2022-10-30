use super::{Args, FromArgs, ToEvent};
use std::{
	cmp::Ordering,
	fmt::{self, Debug, Display, Formatter},
	hash::{Hash, Hasher},
	io::{Error, ErrorKind, Result},
	marker::PhantomData,
	num::NonZeroU32,
};

#[repr(transparent)]
pub struct Id<T>(NonZeroU32, PhantomData<fn(T) -> T>);

impl<T> Id<T> {
	pub fn new(id: u32) -> Option<Self> {
		Some(Self(NonZeroU32::new(id)?, PhantomData))
	}

	pub fn cast<U>(self) -> Id<U> {
		Id(self.0, PhantomData)
	}

	#[doc(hidden)]
	pub fn into_usize(self) -> usize {
		self.0.get() as usize
	}
}

impl<T> Copy for Id<T> {}

impl<T> Clone for Id<T> {
	fn clone(&self) -> Self {
		*self
	}
}

impl<T> Debug for Id<T> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.debug_tuple("Id").field(&self.0).finish()
	}
}

impl<T> Display for Id<T> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		Display::fmt(&self.0, f)
	}
}

impl<T> Hash for Id<T> {
	fn hash<H: Hasher>(&self, hasher: &mut H) {
		self.0.hash(hasher)
	}
}

impl<T> PartialEq for Id<T> {
	fn eq(&self, rhs: &Self) -> bool {
		self.0 == rhs.0
	}
}

impl<T> Eq for Id<T> {}

impl<T> PartialOrd for Id<T> {
	fn partial_cmp(&self, rhs: &Self) -> Option<Ordering> {
		self.0.partial_cmp(&rhs.0)
	}
}

impl<T> Ord for Id<T> {
	fn cmp(&self, rhs: &Self) -> Ordering {
		self.0.cmp(&rhs.0)
	}
}

impl<T> From<Id<T>> for u32 {
	fn from(id: Id<T>) -> Self {
		id.0.get()
	}
}

impl<'a, T> FromArgs<'a> for Id<T> {
	fn from_args(args: &mut Args<'a>) -> Result<Self> {
		match <Option<Self>>::from_args(args)? {
			Some(arg) => Ok(arg),
			None => Err(Error::new(ErrorKind::InvalidInput, "ID may not be null")),
		}
	}
}

impl<'a, T> FromArgs<'a> for Option<Id<T>> {
	fn from_args(args: &mut Args<'a>) -> Result<Self> {
		u32::from_args(args).map(Id::new)
	}
}

impl<T> ToEvent for Id<T> {
	fn encoded_len(&self) -> u16 {
		1
	}

	fn encode(&self, event: &mut super::Event<'_>) {
		event.write(self.0.get())
	}
}

impl<T> ToEvent for Option<Id<T>> {
	fn encoded_len(&self) -> u16 {
		1
	}

	fn encode(&self, event: &mut super::Event<'_>) {
		event.write(self.map_or(0, |id| id.0.get()))
	}
}
