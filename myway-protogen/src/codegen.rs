use crate::types::{ArgType, Interface, Protocol};
use std::{
	fmt::{self, Display, Formatter, Write as _},
	io::{Result, Write},
};

pub(crate) fn emit_protocol(protocol: &Protocol<'_>, dest: &mut impl Write) -> Result<()> {
	if let Some(c) = protocol.copyright {
		writeln!(dest, "// Copyright of the protocol specification:")?;
		write_multiline(dest, "// ", [c])?;
	}
	if let Some(desc) = protocol.desc {
		write_multiline(dest, "//! ", [desc.summary, desc.description])?;
	}
	for iface in &protocol.interfaces {
		emit_interface(dest, iface)?;
	}
	Ok(())
}

fn emit_interface(dest: &mut impl Write, iface: &Interface) -> Result<()> {
	if let Some(desc) = iface.desc {
		write_multiline(dest, "/// ", [desc.summary, desc.description])?;
	}

	// requests, as a trait of handlers
	let type_name = RustName(iface.name);
	writeln!(dest, "pub mod {} {{", iface.name)?;
	writeln!(dest, "\tuse crate::protocol::{{Fixed, FromArgs}};")?;
	writeln!(dest, "\tuse std::{{io::{{self, ErrorKind, Result}}, num::NonZeroU32, os::unix::io::OwnedFd}};")?;
	writeln!(dest, "\tpub trait {type_name}Requests: Sized {{")?;
	for req in &iface.requests {
		if let Some(desc) = req.desc {
			write_multiline(dest, "\t\t/// ", [desc.summary, desc.description])?;
			writeln!(dest, "\t\t///")?;
		}
		writeln!(dest, "\t\t/// # Request Arguments")?;
		writeln!(dest, "\t\t///")?;
		for arg in &req.args {
			writeln!(dest, "\t\t/// - `{}`: {}", arg.name, arg.summary.unwrap_or("(no summary available)"))?;
		}
		write!(dest, "\t\tfn handle_{}(", req.name)?;
		if req.kind == Some("destructor") {
			write!(dest, "self")?;
		} else {
			write!(dest, "&mut self")?;
		}
		for arg in &req.args {
			write!(dest, ", {}: {}", arg.name, RustType(arg.ty))?;
		}
		writeln!(dest, ") -> Result<()>;")?;
	}
	writeln!(dest, "\t\t#[allow(clippy::match_single_binding)]")?;
	writeln!(dest, "\t\tfn handle_request(this: &mut Option<Self>, opcode: u16, args: &[u32]) -> Result<()> {{")?;
	writeln!(dest, "\t\t\tmatch opcode {{")?;
	for (i, req) in iface.requests.iter().enumerate() {
		writeln!(dest, "\t\t\t\t{i} => {{")?;
		for arg in &req.args {
			writeln!(dest, "\t\t\t\t\tlet ({}, args) = <{}>::split(args)?;", arg.name, RustType(arg.ty))?;
		}
		writeln!(dest, "\t\t\t\t\tif !args.is_empty() {{")?;
		writeln!(dest, "\t\t\t\t\t\treturn Err(io::Error::new(ErrorKind::InvalidInput, \"too many args\"));")?;
		writeln!(dest, "\t\t\t\t\t}}")?;
		if req.kind == Some("destructor") {
			write!(dest, "\t\t\t\t\tthis.take().unwrap().handle_{}(", req.name)?;
		} else {
			write!(dest, "\t\t\t\t\tthis.as_mut().unwrap().handle_{}(", req.name)?;
		}
		for arg in &req.args {
			write!(dest, "{}, ", arg.name)?;
		}
		writeln!(dest, ")")?;
		writeln!(dest, "\t\t\t\t}},")?;
	}
	// suppress unused_variables for `this` and `args` without enabling the lint for the entire function
	writeln!(dest, "\t\t\t\t_ => {{ let _ = (this, args); Err(ErrorKind::InvalidInput.into()) }},")?;
	writeln!(dest, "\t\t\t}}")?;
	writeln!(dest, "\t\t}}")?;
	writeln!(dest, "\t}}")?;

	for en in &iface.enums {
		let name = RustName(en.name);
		if let Some(desc) = en.desc {
			write_multiline(dest, "\t/// ", [desc.summary, desc.description])?;
		}
		writeln!(dest, "\t#[repr(u32)]")?;
		writeln!(dest, "\t#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]")?;
		writeln!(dest, "\tpub enum {name} {{")?;
		for ent in &en.entries {
			if let Some(doc) = ent.summary {
				writeln!(dest, "\t\t/// {doc}")?;
			}
			write!(dest, "\t\t{} = ", RustName(ent.name))?;
			if ent.value_is_hex {
				writeln!(dest, "{:#x},", ent.value)?;
			} else {
				writeln!(dest, "{},", ent.value)?;
			}
		}
		writeln!(dest, "\t}}")?;
		writeln!(dest, "\timpl<'a> FromArgs<'a> for {name} {{")?;
		writeln!(dest, "\t\tfn split(args: &'a [u32]) -> Result<(Self, &'a [u32])> {{")?;
		writeln!(dest, "\t\t\tlet (arg, rest) = u32::split(args)?;")?;
		writeln!(dest, "\t\t\tlet arg = match arg {{")?;
		for ent in &en.entries {
			writeln!(dest, "\t\t\t\t{} => Self::{},", ent.value, RustName(ent.name))?;
		}
		writeln!(dest, "\t\t\t\t_ => return Err(io::Error::new(ErrorKind::InvalidInput, \"invalid {name}\")),")?;
		writeln!(dest, "\t\t\t}};")?; // match
		writeln!(dest, "\t\t\tOk((arg, rest))")?;
		writeln!(dest, "\t\t}}")?; // fn
		writeln!(dest, "\t}}")?; // trait impl
	}

	writeln!(dest, "}}")?;
	Ok(())
}

fn write_multiline<'a>(dest: &mut impl Write, prefix: &str, parts: impl IntoIterator<Item = &'a str>) -> Result<()> {
	let mut first = true;
	for part in parts {
		if part.is_empty() {
			continue;
		}
		if !first {
			writeln!(dest, "{}", prefix.trim_end())?;
		}
		first = false;
		for line in part.lines().map(str::trim) {
			if line.is_empty() {
				writeln!(dest, "{}", prefix.trim_end())?;
			} else {
				writeln!(dest, "{prefix}{line}")?;
			}
		}
	}
	Ok(())
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct RustName<'a>(&'a str);

impl Display for RustName<'_> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		// some args use `enum="{iface}.{enum_name}"` to refer to enums from another interface
		let wl_name = if let Some((iface, ty)) = self.0.split_once('.') {
			f.write_str("super::")?;
			f.write_str(iface)?;
			f.write_str("::")?;
			ty
		} else {
			self.0
		};
		// enum wl_output.transform members "90", "180", and "270" are not valid identifiers
		if wl_name.chars().next().ok_or(fmt::Error)?.is_numeric() {
			f.write_char('_')?;
		}
		for word in wl_name.split('_') {
			let mut chars = word.chars();
			let first = chars.next().unwrap_or_else(|| panic!("empty word in wayland name {:?}", self.0));
			for c in first.to_uppercase() {
				f.write_char(c)?;
			}
			f.write_str(chars.as_str())?;
		}
		Ok(())
	}
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct RustType<'a>(ArgType<'a>);

impl Display for RustType<'_> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		match self.0 {
			ArgType::Int => f.write_str("i32"),
			ArgType::Uint => f.write_str("u32"),
			ArgType::Enum(wl_name) => RustName(wl_name).fmt(f),
			ArgType::Fixed => f.write_str("Fixed"),
			ArgType::String { nullable: false } => f.write_str("&str"),
			ArgType::String { nullable: true } => f.write_str("Option<&str>"),
			ArgType::Object { interface: _, nullable: false } => f.write_str("NonZeroU32"),
			ArgType::Object { interface: _, nullable: true } => f.write_str("Option<NonZeroU32>"),
			ArgType::NewId { interface: _ } => f.write_str("NonZeroU32"),
			ArgType::Array => f.write_str("&[u32]"),
			ArgType::Fd => f.write_str("OwnedFd"),
		}
	}
}
