use crate::{
	client::SendHalf,
	object_map::VacantEntry,
	protocol::{
		wl_callback::WlCallback,
		wl_display::WlDisplay,
		wl_registry::WlRegistry,
		wl_shm::{self, WlShm},
		wl_shm_pool::WlShmPool,
		AnyObject, Fd, Id,
	},
};
use log::info;
use nix::sys::mman::{mmap, mremap, MRemapFlags, MapFlags, ProtFlags};
use std::{
	ffi::c_void,
	io::{Error, ErrorKind, Result},
	os::unix::io::AsRawFd,
	ptr,
};

#[derive(Debug)]
pub struct Display;

impl WlDisplay for Display {
	fn handle_sync(&mut self, client: &mut SendHalf<'_>, callback: VacantEntry<'_, Callback>) -> Result<()> {
		info!("wl_display.sync(callback={:?})", callback.id());
		let id = callback.id();
		callback.insert(Callback).take().send_done(id, client, 0)
	}

	fn handle_get_registry(&mut self, client: &mut SendHalf<'_>, registry: VacantEntry<'_, Registry>) -> Result<()> {
		info!("wl_display.get_registry(registry={:?})", registry.id());
		let registry = registry.insert(Registry);
		registry.send_globals(registry.id(), client)
	}
}

#[derive(Debug)]
pub struct Callback;

impl WlCallback for Callback {}

#[derive(Debug)]
pub struct Registry;

impl Registry {
	fn send_globals(&self, self_id: Id<Self>, client: &mut SendHalf<'_>) -> Result<()> {
		self.send_global(self_id, client, 0, "wl_shm", 5)?;
		Ok(())
	}
}

impl WlRegistry for Registry {
	fn handle_bind(
		&mut self,
		client: &mut SendHalf<'_>,
		name: u32,
		interface: &str,
		version: u32,
		id: VacantEntry<'_, AnyObject>,
	) -> Result<()> {
		info!("wl_registry.bind(name={name:?}, interface={interface:?}, version={version:?}, id={:?})", id.id());
		assert!(name == 0, "TODO implement more globals");
		if interface != "wl_shm" {
			return Err(Error::new(
				ErrorKind::InvalidInput,
				"cannot bind to name 0 (ShmGlobal) as interface {iterface:?}",
			));
		}
		if version != 1 {
			return Err(Error::new(ErrorKind::InvalidInput, "ShmGlobal does not implement wl_shm version {version}"));
		}
		let shm = id.downcast().insert(ShmGlobal);
		shm.send_formats(shm.id(), client)
	}
}

#[derive(Debug)]
pub struct ShmGlobal;

impl ShmGlobal {
	fn send_formats(&self, self_id: Id<Self>, client: &mut SendHalf<'_>) -> Result<()> {
		self.send_format(self_id, client, wl_shm::Format::Argb8888)?;
		self.send_format(self_id, client, wl_shm::Format::Xrgb8888)?;
		Ok(())
	}
}

impl WlShm for ShmGlobal {
	fn handle_create_pool(
		&mut self,
		_client: &mut SendHalf<'_>,
		id: VacantEntry<'_, ShmPool>,
		fd: Fd,
		size: i32,
	) -> Result<()> {
		info!("wl_shm.create_pool(id={:?}, fd={fd:?}, size={size:?})", id.id());
		let size = match size.try_into() {
			Ok(n) => n,
			Err(_) => {
				return Err(Error::new(ErrorKind::InvalidInput, "size must be nonnegative"));
			},
		};
		// XXX does calling mmap have safety preconditions separate from safely using the new memory?
		let ptr =
			match unsafe { mmap(ptr::null_mut(), size, ProtFlags::PROT_READ, MapFlags::MAP_SHARED, fd.as_raw_fd(), 0) }
			{
				Ok(ptr) => ptr,
				Err(err) => {
					return Err(Error::new(ErrorKind::InvalidInput, format!("mapping file descriptor failed: {err}")));
				},
			};
		id.insert(ShmPool { ptr, size });
		Ok(())
	}
}

#[derive(Debug)]
pub struct ShmPool {
	ptr: *mut c_void,
	size: usize,
}

impl WlShmPool for ShmPool {
	fn handle_create_buffer(
		&mut self,
		_client: &mut SendHalf<'_>,
		id: VacantEntry<'_, AnyObject>,
		offset: i32,
		width: i32,
		height: i32,
		stride: i32,
		format: wl_shm::Format,
	) -> Result<()> {
		info!(
			"wl_shm_pool.create_buffer(id={:?}, offset={offset:?}, width={width:?}, height={height:?}, \
			 stride={stride:?}, format={format:?})",
			id.id(),
		);
		todo!()
	}

	fn handle_destroy(self, _client: &mut SendHalf<'_>) -> Result<()> {
		info!("wl_shm_pool.destroy()");
		Ok(())
	}

	fn handle_resize(&mut self, _client: &mut SendHalf<'_>, size: i32) -> Result<()> {
		info!("wl_shm_pool.resize(size={size:?})");
		let size = match size.try_into() {
			Ok(n) => n,
			Err(_) => {
				return Err(Error::new(ErrorKind::InvalidInput, "size must be nonnegative"));
			},
		};
		// XXX does calling mremap have safety preconditions separate from safely using the new memory?
		let ptr = match unsafe { mremap(self.ptr, self.size, size, MRemapFlags::MREMAP_MAYMOVE, None) } {
			Ok(ptr) => ptr,
			Err(err) => {
				return Err(Error::new(ErrorKind::InvalidInput, format!("remapping file descriptor failed: {err}")));
			},
		};
		self.ptr = ptr;
		self.size = size;
		Ok(())
	}
}
