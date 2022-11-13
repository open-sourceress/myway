use log::warn;
use nix::sys::{
	mman::{mmap, mremap, munmap, MRemapFlags, MapFlags, ProtFlags},
	stat::fstat,
};
use std::{
	ffi::c_void,
	io::{Error, ErrorKind, Result},
	os::unix::{io::OwnedFd, prelude::AsRawFd},
	ptr,
};

/// A block of memory shared with a Wayland client, from which buffers can be created.
#[derive(Debug)]
pub struct ShmBlock {
	/// File descriptor that was initially mmap'd to create this shared memory.
	fd: OwnedFd,
	/// Pointer that the memory is currently mapped at.
	ptr: *mut c_void,
	/// Size of the memory block, in bytes.
	length: usize,
}

impl ShmBlock {
	/// Create a [`ShmBlock`] by memory-mapping a file descriptor.
	pub fn new(fd: OwnedFd, length: usize) -> Result<Self> {
		let stat = fstat(fd.as_raw_fd())?;
		if stat.st_size.try_into().map_or(true, |st_size: usize| st_size < length) {
			return Err(Error::new(
				ErrorKind::InvalidInput,
				format!("cannot map {length} bytes from a file of length {}", stat.st_size),
			));
		}
		// Safety: addr NULL ensures no other memory will be unmapped
		// XXX does mmap have any other safety requirements?
		let ptr =
			unsafe { mmap(ptr::null_mut(), length, ProtFlags::PROT_READ, MapFlags::MAP_SHARED, fd.as_raw_fd(), 0)? };
		Ok(Self { fd, ptr, length })
	}

	pub fn grow(&mut self, new_length: usize) -> Result<()> {
		if new_length < self.length {
			return Err(Error::new(
				ErrorKind::InvalidInput,
				format!("cannot shrink shared memory from {} bytes to {new_length} bytes", self.length),
			));
		}
		let stat = fstat(self.fd.as_raw_fd())?;
		if stat.st_size.try_into().map_or(true, |st_size: usize| st_size < new_length) {
			return Err(Error::new(
				ErrorKind::InvalidInput,
				format!("cannot map {new_length} bytes from a file of length {}", stat.st_size),
			));
		}

		unsafe {
			// Safety: accessing the mapped memory requires &self, so holding an &mut self ensures the memory is not
			// currently being accessed
			self.ptr = mremap(self.ptr, self.length, new_length, MRemapFlags::MREMAP_MAYMOVE, None)?;
			self.length = new_length;
		}
		Ok(())
	}

	pub fn as_ptr(&self) -> *const u8 {
		self.ptr.cast()
	}

	pub fn len(&self) -> usize {
		self.length
	}
}

impl Drop for ShmBlock {
	fn drop(&mut self) {
		// Safety: every referent holds a reference to this object, so no references to the mapped memory exist when
		// this destructor is run
		match unsafe { munmap(self.ptr, self.length) } {
			Ok(()) => (),
			Err(err) => warn!("munmap({:p}, {}) failed: {err}", self.ptr, self.length),
		}
	}
}
