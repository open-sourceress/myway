#[allow(unused_imports, dead_code, clippy::enum_variant_names)]
pub mod wayland {
	include!(concat!(env!("OUT_DIR"), "/wayland_protocol.rs"));
}
