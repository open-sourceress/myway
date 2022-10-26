use roxmltree::Document;
use std::{
	fs::{self, File},
	io::{BufWriter, Error, ErrorKind, Result, Write},
	path::Path,
};

macro_rules! bail {
	($pat:literal $($args:tt)*) => {
		return Err(crate::Error::new(crate::ErrorKind::InvalidData, format!($pat $($args)*)))
	};
}

macro_rules! ensure {
	($cond:expr, $pat:literal $($args:tt)*) => {
		if !$cond {
			bail!($pat $($args)*);
		}
	};
}

mod build_tree;
mod codegen;
mod types;

pub fn generate(schema_path: impl AsRef<Path>, code_path: impl AsRef<Path>) -> Result<()> {
	let schema = fs::read_to_string(schema_path)?;
	let schema = Document::parse(&schema).map_err(|err| Error::new(ErrorKind::InvalidData, err))?;
	let mut output = BufWriter::new(File::create(code_path)?);
	let tree = build_tree::build_protocol(&schema)?;
	codegen::emit_protocol(&tree, &mut output)?;
	output.flush()?;
	Ok(())
}
