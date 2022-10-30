use crate::{
	client::SendHalf,
	objects::VacantEntry,
	protocol::{wl_callback::WlCallback, wl_display::WlDisplay, wl_registry::WlRegistry, Object},
};
use log::info;
use std::io::Result;

#[derive(Debug)]
pub struct Display;

impl WlDisplay for Display {
	fn handle_sync(&mut self, client: &mut SendHalf<'_>, callback: VacantEntry<'_, Callback>) -> Result<()> {
		info!("wl_display.sync(callback={:?})", callback.id());
		client.submit(callback.id().into(), 0, &[/* seq: TODO fill this actually */ 0])
	}

	fn handle_get_registry(&mut self, client: &mut SendHalf<'_>, registry: VacantEntry<'_, Registry>) -> Result<()> {
		info!("wl_display.get_registry(registry={:?})", registry.id());
		let registry = registry.insert(Registry);
		client.submit(registry.id().into(), /* global */ 0, &[
			0,  // name: uint
			14, // interface: string (len)
			u32::from_ne_bytes(*b"wl_c"),
			u32::from_ne_bytes(*b"ompo"),
			u32::from_ne_bytes(*b"sito"),
			u32::from_ne_bytes(*b"r\0\0\0"),
			5, // version: uint
		])?;
		client.submit(registry.id().into(), 0, &[
			1,
			7,
			u32::from_ne_bytes(*b"wl_s"),
			u32::from_ne_bytes(*b"hm\0\0"),
			1,
		])?;
		client.submit(registry.id().into(), 0, &[
			2,  // name: uint
			23, // interface: string (len)
			u32::from_ne_bytes(*b"wl_d"),
			u32::from_ne_bytes(*b"ata_"),
			u32::from_ne_bytes(*b"devi"),
			u32::from_ne_bytes(*b"ce_m"),
			u32::from_ne_bytes(*b"anag"),
			u32::from_ne_bytes(*b"er\0\0"),
			3, // version: uint
		])?;
		Ok(())
	}
}

#[derive(Debug)]
pub struct Callback;

impl WlCallback for Callback {}

#[derive(Debug)]
pub struct Registry;

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
