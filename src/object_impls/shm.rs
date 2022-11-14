use crate::{
	client::SendHalf,
	object_map::VacantEntry,
	protocol::{
		wl_buffer::WlBuffer,
		wl_shm::{Format, WlShm},
		wl_shm_pool::WlShmPool,
		Fd, Id,
	},
	shm::ShmBlock,
};
use log::info;
use std::{
	cell::RefCell,
	io::{Error, ErrorKind, Result},
	rc::Rc,
};

#[derive(Debug)]
pub struct ShmGlobal;

impl ShmGlobal {
	pub(super) fn send_formats(&self, self_id: Id<Self>, client: &mut SendHalf<'_>) -> Result<()> {
		self.send_format(self_id, client, Format::Argb8888)?;
		self.send_format(self_id, client, Format::Xrgb8888)?;
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
		let block = ShmBlock::new(fd, size)?;
		id.insert(ShmPool(Rc::new(RefCell::new(block))));
		Ok(())
	}
}

#[derive(Debug)]
pub struct ShmPool(Rc<RefCell<ShmBlock>>);

impl WlShmPool for ShmPool {
	fn handle_create_buffer(
		&mut self,
		_client: &mut SendHalf<'_>,
		id: VacantEntry<'_, ShmBuffer>,
		offset: i32,
		width: i32,
		height: i32,
		stride: i32,
		format: Format,
	) -> Result<()> {
		info!(
			"wl_shm_pool.create_buffer(id={:?}, offset={offset:?}, width={width:?}, height={height:?}, \
			 stride={stride:?}, format={format:?})",
			id.id(),
		);
		let offset = offset
			.try_into()
			.map_err(|_| Error::new(ErrorKind::InvalidInput, format!("buffer offset {offset} is negative")))?;
		let width = width
			.try_into()
			.map_err(|_| Error::new(ErrorKind::InvalidInput, format!("buffer width {width} is negative")))?;
		let height = height
			.try_into()
			.map_err(|_| Error::new(ErrorKind::InvalidInput, format!("buffer height {height} is negative")))?;
		let stride = stride
			.try_into()
			.map_err(|_| Error::new(ErrorKind::InvalidInput, format!("buffer stride {stride} is negative")))?;
		if !matches!(format, Format::Argb8888 | Format::Xrgb8888) {
			return Err(Error::new(ErrorKind::InvalidInput, "unsupported format"));
		}
		id.insert(ShmBuffer { memory: self.0.clone(), offset, width, height, stride, format });
		Ok(())
	}

	fn handle_destroy(self, _client: &mut SendHalf<'_>) -> Result<()> {
		info!("wl_shm_pool.destroy()");
		Ok(())
	}

	fn handle_resize(&mut self, _client: &mut SendHalf<'_>, size: i32) -> Result<()> {
		info!("wl_shm_pool.resize(size={size:?})");
		match size.try_into() {
			Ok(size) => self.0.borrow_mut().grow(size),
			Err(_) => Err(Error::new(ErrorKind::InvalidInput, "size is negative")),
		}
	}
}

#[derive(Clone, Debug)]
pub struct ShmBuffer {
	pub(super) memory: Rc<RefCell<ShmBlock>>,
	pub(super) offset: u32,
	#[allow(dead_code)]
	pub(super) width: u32,
	pub(super) height: u32,
	pub(super) stride: u32,
	#[allow(dead_code)]
	pub(super) format: Format,
}

impl WlBuffer for ShmBuffer {
	fn handle_destroy(self, _client: &mut SendHalf<'_>) -> Result<()> {
		info!("wl_buffer.destroy()");
		Ok(())
	}
}
