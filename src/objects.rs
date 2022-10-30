use crate::{
	client, object_impls,
	protocol::{Args, Id, Object},
};
use std::{
	fmt::{self},
	io::{Error, ErrorKind, Result},
	mem::MaybeUninit,
	ops::{Deref, DerefMut},
};

pub trait ObjectType: Sized {
	fn upcast(self) -> Object;
	fn downcast(object: Object) -> Option<Self>;
	fn downcast_ref(object: &Object) -> Option<&Self>;
	fn downcast_mut(object: &mut Object) -> Option<&mut Self>;
}

impl ObjectType for Object {
	fn upcast(self) -> Object {
		self
	}

	fn downcast(object: Object) -> Option<Self> {
		Some(object)
	}

	fn downcast_ref(object: &Object) -> Option<&Self> {
		Some(object)
	}

	fn downcast_mut(object: &mut Object) -> Option<&mut Self> {
		Some(object)
	}
}

pub struct Objects {
	vec: Vec<Option<Object>>,
}

impl Objects {
	pub fn new() -> Self {
		Self { vec: Vec::with_capacity(2) } // ensure we at least have the capacity for the Display at ID 1
	}

	pub fn insert<T: ObjectType>(&mut self, id: Id<T>, obj: T) -> Result<OccupiedEntry<'_, T>> {
		let [entry] = self.get_many_mut([id.cast()])?;
		Ok(entry.into_vacant()?.downcast().insert(obj))
	}

	pub fn get_many_mut<const N: usize>(&mut self, ids: [Id<Object>; N]) -> Result<[Entry<'_, Object>; N]> {
		let mut new_len = self.vec.len();
		for (i, &id) in ids.iter().enumerate() {
			for &id2 in &ids[..i] {
				if id == id2 {
					return Err(Error::new(ErrorKind::InvalidInput, format!("requested id {id} multiple times")));
				}
			}
			new_len = new_len.max(id.into_usize() + 1);
		}
		// new_len starts at `self.vec.len()` and only goes up, so this will never shrink the vec
		self.vec.resize_with(new_len, || None);
		let ret = unsafe {
			let (slice_ptr, slice_len) = (self.vec.as_mut_ptr(), self.vec.len());
			// Safety: fully uninitialized is a valid bit-pattern for [MaybeUninit<T>; N]
			let mut ret: [MaybeUninit<Entry<'_, Object>>; N] = MaybeUninit::uninit().assume_init();
			for ret_idx in 0..N {
				let id = ids[ret_idx];
				let object_idx = id.into_usize();
				debug_assert!(object_idx < slice_len); // This is ensured by the resize_with above, so only debug_assert

				// Safety: resize_with ensures that object_ptr is within the backing allocation of `self.vec`, and the
				// nested loop ensures no index is present twice and so at most one mutable reference is created for
				// each element of the slice.
				let object_ref = &mut *slice_ptr.add(object_idx);
				ret[ret_idx].write(Entry::new(id, object_ref));
			}
			// Safety: every slot in `ret` was initialized by the loop above
			ret.map(|slot| slot.assume_init())
		};
		Ok(ret)
	}

	pub fn dispatch_request(
		&mut self,
		client: &mut client::SendHalf<'_>,
		id: Id<Object>,
		opcode: u16,
		args: Args<'_>,
	) -> Result<()> {
		let handler = match self.vec.get(id.into_usize()) {
			Some(Some(Object::Display(_))) => object_impls::Display::handle_request,
			Some(Some(Object::Callback(_))) => object_impls::Callback::handle_request,
			Some(Some(Object::Registry(_))) => object_impls::Registry::handle_request,
			None | Some(None) => return Ok(()),
		};
		handler(self, id, client, opcode, args)
	}
}

impl fmt::Debug for Objects {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("Objects ")?;
		let mut m = f.debug_map();
		for (i, slot) in self.vec.iter().skip(1).enumerate() {
			m.entry(&i, slot);
		}
		m.finish()
	}
}

#[derive(Debug)]
pub enum Entry<'a, T> {
	Occupied(OccupiedEntry<'a, T>),
	Vacant(VacantEntry<'a, T>),
}

impl<'a> Entry<'a, Object> {
	fn new(id: Id<Object>, slot: &'a mut Option<Object>) -> Self {
		if slot.is_some() {
			Self::Occupied(OccupiedEntry { id, slot })
		} else {
			Self::Vacant(VacantEntry { id, slot })
		}
	}
}

impl<'a, T> Entry<'a, T> {
	pub fn into_occupied(self) -> Result<OccupiedEntry<'a, T>> {
		match self {
			Self::Occupied(entry) => Ok(entry),
			Self::Vacant(entry) => Err(Error::new(ErrorKind::NotFound, format!("id {} does not exist", entry.id))),
		}
	}

	pub fn into_vacant(self) -> Result<VacantEntry<'a, T>> {
		match self {
			Self::Occupied(entry) => Err(Error::new(ErrorKind::AlreadyExists, format!("id {} exists", entry.id))),
			Self::Vacant(entry) => Ok(entry),
		}
	}
}

#[derive(Debug)]
pub struct OccupiedEntry<'a, T> {
	id: Id<T>,
	slot: &'a mut Option<Object>,
}

impl<'a> OccupiedEntry<'a, Object> {
	pub fn downcast<T: ObjectType>(self) -> Result<OccupiedEntry<'a, T>> {
		if T::downcast_ref(&*self).is_some() {
			Ok(OccupiedEntry { id: self.id.cast(), slot: self.slot })
		} else {
			Err(Error::new(ErrorKind::InvalidInput, format!("ID {} is not the correct type", self.id)))
		}
	}
}

impl<'a, T: ObjectType> OccupiedEntry<'a, T> {
	pub fn id(&self) -> Id<T> {
		self.id
	}

	#[allow(dead_code)]
	pub fn take(self) -> T {
		match self.slot.take() {
			Some(obj) => T::downcast(obj).unwrap(),
			None => panic!("OccupiedEntry created from empty slot (id={})", self.id),
		}
	}
}

impl<'a, T: ObjectType> Deref for OccupiedEntry<'a, T> {
	type Target = T;

	fn deref(&self) -> &Self::Target {
		match self.slot.as_ref() {
			Some(obj) => T::downcast_ref(obj).unwrap(),
			None => panic!("OccupiedEntry created from empty slot (id={})", self.id),
		}
	}
}

impl<'a, T: ObjectType> DerefMut for OccupiedEntry<'a, T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		match self.slot.as_mut() {
			Some(obj) => T::downcast_mut(obj).unwrap(),
			None => panic!("OccupiedEntry created from empty slot (id={})", self.id),
		}
	}
}

#[derive(Debug)]
pub struct VacantEntry<'a, T> {
	id: Id<T>,
	slot: &'a mut Option<Object>,
}

impl<'a> VacantEntry<'a, Object> {
	pub fn downcast<T: ObjectType>(self) -> VacantEntry<'a, T> {
		VacantEntry { id: self.id.cast(), slot: self.slot }
	}
}

impl<'a, T: ObjectType> VacantEntry<'a, T> {
	pub fn id(&self) -> Id<T> {
		self.id
	}

	pub fn insert(self, obj: T) -> OccupiedEntry<'a, T> {
		debug_assert!(self.slot.is_none(), "Vacant Entry created from occupied slot (id={})", self.id);
		*self.slot = Some(obj.upcast());
		OccupiedEntry { id: self.id, slot: self.slot }
	}
}
