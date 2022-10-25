use std::{env, io::Result, path::PathBuf};

fn main() -> Result<()> {
	let mut path = PathBuf::from(env::var_os("OUT_DIR").unwrap());
	path.push("wayland_protocol.rs");
	myway_protogen::generate("/usr/share/wayland/wayland.xml", path)
}
