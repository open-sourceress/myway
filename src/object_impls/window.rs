use super::{shm::ShmBuffer, Callback};
use crate::{
	client::SendHalf,
	object_map::{OccupiedEntry, VacantEntry},
	protocol::{
		wl_compositor::WlCompositor,
		wl_output::Transform,
		wl_region::WlRegion,
		wl_surface::WlSurface,
		xdg_popup::XdgPopup,
		xdg_positioner::{Gravity, XdgPositioner},
		xdg_surface::XdgSurface,
		xdg_toplevel::XdgToplevel,
		xdg_wm_base::XdgWmBase,
		AnyObject,
	},
	windows::{PopupRole, ToplevelRole, WindowRole},
};
use log::info;
use std::{
	cell::{RefCell, RefMut},
	io::{Error, ErrorKind, Result},
	rc::Rc,
};

#[derive(Debug)]
pub struct Compositor;

impl WlCompositor for Compositor {
	fn handle_create_surface(&mut self, _client: &mut SendHalf<'_>, surface: VacantEntry<'_, Surface>) -> Result<()> {
		info!("wl_compositor.create_surface(surface={})", surface.id());
		surface.insert(Surface::default());
		Ok(())
	}

	fn handle_create_region(&mut self, _client: &mut SendHalf<'_>, slot: VacantEntry<'_, Region>) -> Result<()> {
		info!("wl_compositor.create_region(region={})", slot.id());
		slot.insert(Region);
		Ok(())
	}
}

#[derive(Debug, Default)]
pub struct Surface {
	current: BufferedSurfaceState,
	pending: BufferedSurfaceState,
	role: Option<Rc<RefCell<WindowRole>>>,
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

#[derive(Debug)]
pub struct WindowManager;

impl XdgWmBase for WindowManager {
	fn handle_destroy(self, _client: &mut SendHalf<'_>) -> Result<()> {
		todo!()
	}

	fn handle_create_positioner(&mut self, _client: &mut SendHalf<'_>, id: VacantEntry<'_, Positioner>) -> Result<()> {
		id.insert(Positioner);
		Ok(())
	}

	fn handle_get_xdg_surface(
		&mut self,
		_client: &mut SendHalf<'_>,
		id: VacantEntry<'_, XdgSurfaceImpl>,
		mut surface: OccupiedEntry<'_, Surface>,
	) -> Result<()> {
		if surface.role.is_some() {
			return Err(Error::new(ErrorKind::InvalidInput, "wl_surface already has an xdg_surface"));
		}
		let role = surface.role.insert(Default::default());
		id.insert(XdgSurfaceImpl(role.clone()));
		Ok(())
	}

	fn handle_pong(&mut self, _client: &mut SendHalf<'_>, _serial: u32) -> Result<()> {
		Ok(())
	}
}

#[derive(Debug)]
pub struct XdgSurfaceImpl(Rc<RefCell<WindowRole>>);

impl XdgSurface for XdgSurfaceImpl {
	fn handle_destroy(self, _client: &mut SendHalf<'_>) -> Result<()> {
		if matches!(*self.0.borrow(), WindowRole::Unassigned) {
			Ok(())
		} else {
			Err(Error::new(ErrorKind::Other, "cannot destroy xdg_surface that has an assigned role"))
		}
	}

	fn handle_get_toplevel(&mut self, _client: &mut SendHalf<'_>, id: VacantEntry<'_, ToplevelObject>) -> Result<()> {
		let mut role = self.0.borrow_mut();
		if matches!(*role, WindowRole::Unassigned) {
			*role = WindowRole::Toplevel(ToplevelRole { title: None, app_id: None });
			id.insert(ToplevelObject(self.0.clone()));
			Ok(())
		} else {
			Err(Error::new(ErrorKind::Other, "xdg_surface already has a role"))
		}
	}

	fn handle_get_popup(
		&mut self,
		_client: &mut SendHalf<'_>,
		id: VacantEntry<'_, PopupObject>,
		_parent: Option<OccupiedEntry<'_, XdgSurfaceImpl>>,
		_positioner: OccupiedEntry<'_, Positioner>,
	) -> Result<()> {
		let mut role = self.0.borrow_mut();
		if matches!(*role, WindowRole::Unassigned) {
			*role = WindowRole::Popup(PopupRole);
			id.insert(PopupObject(self.0.clone()));
			Ok(())
		} else {
			Err(Error::new(ErrorKind::Other, "xdg_surface already has a role"))
		}
	}

	fn handle_set_window_geometry(
		&mut self,
		_client: &mut SendHalf<'_>,
		_x: i32,
		_y: i32,
		_width: i32,
		_height: i32,
	) -> Result<()> {
		todo!()
	}

	fn handle_ack_configure(&mut self, _client: &mut SendHalf<'_>, _serial: u32) -> Result<()> {
		todo!()
	}
}

#[derive(Debug)]
pub struct Positioner;

impl XdgPositioner for Positioner {
	fn handle_destroy(self, _client: &mut SendHalf<'_>) -> Result<()> {
		todo!()
	}

	fn handle_set_size(&mut self, _client: &mut SendHalf<'_>, _width: i32, _height: i32) -> Result<()> {
		todo!()
	}

	fn handle_set_anchor_rect(
		&mut self,
		_client: &mut SendHalf<'_>,
		_x: i32,
		_y: i32,
		_width: i32,
		_height: i32,
	) -> Result<()> {
		todo!()
	}

	fn handle_set_anchor(
		&mut self,
		_client: &mut SendHalf<'_>,
		_anchor: crate::protocol::xdg_positioner::Anchor,
	) -> Result<()> {
		todo!()
	}

	fn handle_set_gravity(&mut self, _client: &mut SendHalf<'_>, _gravity: Gravity) -> Result<()> {
		todo!()
	}

	fn handle_set_constraint_adjustment(
		&mut self,
		_client: &mut SendHalf<'_>,
		_constraint_adjustment: u32,
	) -> Result<()> {
		todo!()
	}

	fn handle_set_offset(&mut self, _client: &mut SendHalf<'_>, _x: i32, _y: i32) -> Result<()> {
		todo!()
	}

	fn handle_set_reactive(&mut self, _client: &mut SendHalf<'_>) -> Result<()> {
		todo!()
	}

	fn handle_set_parent_size(
		&mut self,
		_client: &mut SendHalf<'_>,
		_parent_width: i32,
		_parent_height: i32,
	) -> Result<()> {
		todo!()
	}

	fn handle_set_parent_configure(&mut self, _client: &mut SendHalf<'_>, _serial: u32) -> Result<()> {
		todo!()
	}
}

#[derive(Debug)]
pub struct ToplevelObject(Rc<RefCell<WindowRole>>);

impl ToplevelObject {
	fn get_mut(&self) -> RefMut<'_, ToplevelRole> {
		RefMut::map(self.0.borrow_mut(), |role| match role {
			WindowRole::Toplevel(tl) => tl,
			_ => unreachable!(),
		})
	}
}

impl XdgToplevel for ToplevelObject {
	fn handle_destroy(self, _client: &mut SendHalf<'_>) -> Result<()> {
		todo!()
	}

	fn handle_set_parent(
		&mut self,
		_client: &mut SendHalf<'_>,
		_parent: Option<OccupiedEntry<'_, ToplevelObject>>,
	) -> Result<()> {
		todo!()
	}

	fn handle_set_title(&mut self, _client: &mut SendHalf<'_>, title: &str) -> Result<()> {
		self.get_mut().title = Some(title.into());
		Ok(())
	}

	fn handle_set_app_id(&mut self, _client: &mut SendHalf<'_>, app_id: &str) -> Result<()> {
		self.get_mut().app_id = Some(app_id.into());
		Ok(())
	}

	fn handle_show_window_menu(
		&mut self,
		_client: &mut SendHalf<'_>,
		_seat: OccupiedEntry<'_, AnyObject>,
		_serial: u32,
		_x: i32,
		_y: i32,
	) -> Result<()> {
		todo!()
	}

	fn handle_move(
		&mut self,
		_client: &mut SendHalf<'_>,
		_seat: OccupiedEntry<'_, AnyObject>,
		_serial: u32,
	) -> Result<()> {
		todo!()
	}

	fn handle_resize(
		&mut self,
		_client: &mut SendHalf<'_>,
		_seat: OccupiedEntry<'_, AnyObject>,
		_serial: u32,
		_edges: crate::protocol::xdg_toplevel::ResizeEdge,
	) -> Result<()> {
		todo!()
	}

	fn handle_set_max_size(&mut self, _client: &mut SendHalf<'_>, _width: i32, _height: i32) -> Result<()> {
		todo!()
	}

	fn handle_set_min_size(&mut self, _client: &mut SendHalf<'_>, _width: i32, _height: i32) -> Result<()> {
		todo!()
	}

	fn handle_set_maximized(&mut self, _client: &mut SendHalf<'_>) -> Result<()> {
		todo!()
	}

	fn handle_unset_maximized(&mut self, _client: &mut SendHalf<'_>) -> Result<()> {
		todo!()
	}

	fn handle_set_fullscreen(
		&mut self,
		_client: &mut SendHalf<'_>,
		_output: Option<OccupiedEntry<'_, AnyObject>>,
	) -> Result<()> {
		todo!()
	}

	fn handle_unset_fullscreen(&mut self, _client: &mut SendHalf<'_>) -> Result<()> {
		todo!()
	}

	fn handle_set_minimized(&mut self, _client: &mut SendHalf<'_>) -> Result<()> {
		todo!()
	}
}

#[derive(Debug)]
pub struct PopupObject(Rc<RefCell<WindowRole>>);

impl XdgPopup for PopupObject {
	fn handle_destroy(self, _client: &mut SendHalf<'_>) -> Result<()> {
		*self.0.borrow_mut() = WindowRole::Unassigned;
		Ok(())
	}

	fn handle_grab(
		&mut self,
		_client: &mut SendHalf<'_>,
		_seat: OccupiedEntry<'_, AnyObject>,
		_serial: u32,
	) -> Result<()> {
		todo!()
	}

	fn handle_reposition(
		&mut self,
		_client: &mut SendHalf<'_>,
		_positioner: OccupiedEntry<'_, Positioner>,
		_token: u32,
	) -> Result<()> {
		todo!()
	}
}
