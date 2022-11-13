use crate::{
	client::SendHalf,
	object_map::{OccupiedEntry, VacantEntry},
	protocol::{
		wl_buffer::WlBuffer,
		wl_callback::WlCallback,
		wl_compositor::WlCompositor,
		wl_display::WlDisplay,
		wl_output::Transform,
		wl_region::WlRegion,
		wl_registry::WlRegistry,
		wl_shm::{Format, WlShm},
		wl_shm_pool::WlShmPool,
		wl_surface::WlSurface,
		AnyObject, Fd, Id,
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
		self.send_global(self_id, client, 0, "wl_shm", 1)?;
		self.send_global(self_id, client, 1, "wl_compositor", 5)?;
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
		match (name, interface, version) {
			(0, "wl_shm", 1) => {
				let shm = id.downcast().insert(ShmGlobal);
				shm.send_formats(shm.id(), client)
			},
			(1, "wl_compositor", 5) => {
				id.downcast().insert(Compositor);
				Ok(())
			},
			_ => Err(Error::new(
				ErrorKind::InvalidInput,
				format!("cannot bind global #{name} as {interface} v{version}"),
			)),
		}
	}
}

#[derive(Debug)]
pub struct ShmGlobal;

impl ShmGlobal {
	fn send_formats(&self, self_id: Id<Self>, client: &mut SendHalf<'_>) -> Result<()> {
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
	memory: Rc<RefCell<ShmBlock>>,
	offset: u32,
	width: u32,
	height: u32,
	stride: u32,
	format: Format,
}

impl WlBuffer for ShmBuffer {
	fn handle_destroy(self, _client: &mut SendHalf<'_>) -> Result<()> {
		info!("wl_buffer.destroy()");
		Ok(())
	}
}

#[derive(Debug)]
pub struct Compositor;

impl WlCompositor for Compositor {
	fn handle_create_surface(&mut self, _client: &mut SendHalf<'_>, surface: VacantEntry<'_, Surface>) -> Result<()> {
		info!("wl_compositor.create_surface(surface={})", surface.id());
		surface.insert(Surface { current: Default::default(), pending: Default::default() });
		Ok(())
	}

	fn handle_create_region(&mut self, _client: &mut SendHalf<'_>, slot: VacantEntry<'_, Region>) -> Result<()> {
		info!("wl_compositor.create_region(region={})", slot.id());
		slot.insert(Region);
		Ok(())
	}
}

#[derive(Debug)]
pub struct Surface {
	current: BufferedSurfaceState,
	pending: BufferedSurfaceState,
}

#[derive(Debug)]
struct BufferedSurfaceState {
	buffer: Option<ShmBuffer>,
	offset: [i32; 2],
	scale: i32,
	transform: Transform,
}

impl Default for BufferedSurfaceState {
	fn default() -> Self {
		Self { buffer: None, offset: [0; 2], scale: 1, transform: Transform::Normal }
	}
}

impl WlSurface for Surface {
	fn handle_destroy(self, _client: &mut SendHalf<'_>) -> Result<()> {
		info!("wl_surface.destroy()");
		Ok(())
	}

	fn handle_attach(
		&mut self,
		_client: &mut SendHalf<'_>,
		buffer: Option<OccupiedEntry<'_, ShmBuffer>>,
		x: i32,
		y: i32,
	) -> Result<()> {
		self.pending.buffer = buffer.as_ref().map(|buffer| (**buffer).clone());
		self.pending.offset = [x, y];
		Ok(())
	}

	fn handle_damage(&mut self, _client: &mut SendHalf<'_>, _x: i32, _y: i32, _width: i32, _height: i32) -> Result<()> {
		Ok(())
	}

	fn handle_frame(&mut self, _client: &mut SendHalf<'_>, callback: VacantEntry<'_, Callback>) -> Result<()> {
		callback.insert(Callback);
		Ok(())
	}

	fn handle_set_opaque_region(
		&mut self,
		_client: &mut SendHalf<'_>,
		_region: Option<OccupiedEntry<'_, Region>>,
	) -> Result<()> {
		todo!()
	}

	fn handle_set_input_region(
		&mut self,
		_client: &mut SendHalf<'_>,
		_region: Option<OccupiedEntry<'_, Region>>,
	) -> Result<()> {
		todo!()
	}

	fn handle_commit(&mut self, _client: &mut SendHalf<'_>) -> Result<()> {
		self.current = std::mem::take(&mut self.pending);

		if let Some(ref buffer) = self.current.buffer {
			let path = format!(
				"/tmp/myway-{pid}-{self:p}-{time}.bin",
				pid = std::process::id(),
				time = std::time::SystemTime::UNIX_EPOCH.elapsed().unwrap().as_secs()
			);
			let mut f = std::fs::File::create(&path).unwrap();

			let buf = unsafe {
				let ptr = buffer.memory.borrow().as_ptr().add(buffer.offset as usize);
				let len = buffer.stride * buffer.height;
				std::slice::from_raw_parts(ptr, len as usize)
			};
			std::io::Write::write_all(&mut f, buf).unwrap();
			info!("surface contents dumped to {path}");
		}

		Ok(())
	}

	fn handle_set_buffer_transform(&mut self, _client: &mut SendHalf<'_>, transform: Transform) -> Result<()> {
		self.pending.transform = transform;
		Ok(())
	}

	fn handle_set_buffer_scale(&mut self, _client: &mut SendHalf<'_>, scale: i32) -> Result<()> {
		self.pending.scale = scale;
		Ok(())
	}

	fn handle_damage_buffer(
		&mut self,
		_client: &mut SendHalf<'_>,
		_x: i32,
		_y: i32,
		_width: i32,
		_height: i32,
	) -> Result<()> {
		todo!()
	}

	fn handle_offset(&mut self, _client: &mut SendHalf<'_>, x: i32, y: i32) -> Result<()> {
		self.pending.offset = [x, y];
		Ok(())
	}
}

#[derive(Debug)]
pub struct Region;

impl WlRegion for Region {
	fn handle_destroy(self, _client: &mut SendHalf<'_>) -> Result<()> {
		Ok(())
	}

	fn handle_add(&mut self, _client: &mut SendHalf<'_>, _x: i32, _y: i32, _width: i32, _height: i32) -> Result<()> {
		Ok(())
	}

	fn handle_subtract(
		&mut self,
		_client: &mut SendHalf<'_>,
		_x: i32,
		_y: i32,
		_width: i32,
		_height: i32,
	) -> Result<()> {
		Ok(())
	}
}
