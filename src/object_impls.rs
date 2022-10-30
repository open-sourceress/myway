use crate::{
	client::SendHalf,
	objects::VacantEntry,
	protocol::{wl_callback::WlCallback, wl_display::WlDisplay, wl_registry::WlRegistry, Id, Object},
};
use log::info;
use std::io::Result;

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
		self.send_global(self_id, client, 0, "wl_compositor", 5)?;
		self.send_global(self_id, client, 1, "wl_shm", 5)?;
		self.send_global(self_id, client, 2, "wl_data_device_manager", 3)?;
		Ok(())
	}
}

impl WlRegistry for Registry {
	fn handle_bind(
		&mut self,
		_client: &mut SendHalf<'_>,
		name: u32,
		interface: &str,
		version: u32,
		id: VacantEntry<'_, Object>,
	) -> Result<()> {
		info!("wl_registry.bind(name={name:?}, interface={interface:?}, version={version:?}, id={:?})", id.id());
		Ok(())
	}
}
