use crate::protocol::wayland::wl_display::WlDisplayRequests;
use log::info;
use std::{io::Result, num::NonZeroU32};

#[derive(Debug)]
pub struct Display;

impl WlDisplayRequests for Display {
	fn handle_sync(&mut self, callback: NonZeroU32) -> Result<()> {
		info!("Display handling sync cb={callback}");
		Ok(())
	}

	fn handle_get_registry(&mut self, registry: NonZeroU32) -> Result<()> {
		info!("Display handling get_registry registry={registry}");
		Ok(())
	}
}
