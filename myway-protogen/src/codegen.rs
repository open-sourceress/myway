use crate::types::{Arg, ArgType, Interface, Protocol};
use std::{
	fmt::{self, Display, Formatter, Write as _},
	io::{Result, Write},
};

static IMPL_TYPES: &[(&str, &str)] = &[
	("wl_display", "crate::object_impls::Display"),
	("wl_callback", "crate::object_impls::Callback"),
	("wl_registry", "crate::object_impls::Registry"),
];

fn impl_of<'a, 'b>(iface: &'b str) -> Option<&'a str> {
	IMPL_TYPES.iter().find(|&&(ifa, _)| ifa == iface).map(|&(_, ty)| ty)
}

pub(crate) fn emit_protocol(protocol: &Protocol<'_>, dest: &mut impl Write) -> Result<()> {
	if let Some(c) = protocol.copyright {
		writeln!(dest, "// Copyright of the protocol specification:")?;
		write_multiline(dest, "// > ", [c])?;
	}
	writeln!(dest, "use crate::objects::{{ObjectType}};")?;
	if let Some(desc) = protocol.desc {
		write_multiline(dest, "//! ", [desc.summary, desc.description])?;
	}
	for iface in &protocol.interfaces {
		emit_interface(dest, iface, impl_of(iface.name))?;
	}
	for &(_, ty) in IMPL_TYPES {
		let bare_ty = ty.rsplit_once(':').map_or(ty, |(_, name)| name);
		writeln!(dest, "impl ObjectType for {ty} {{")?;
		writeln!(dest, "\tfn upcast(self) -> Object {{")?;
		writeln!(dest, "\t\tObject::{bare_ty}(self)")?;
		writeln!(dest, "\t}}")?;
		for (func, ref_sigil) in [("downcast", ""), ("downcast_ref", "&"), ("downcast_mut", "&mut ")] {
			writeln!(dest, "\tfn {func}(object: {ref_sigil}Object) -> Option<{ref_sigil}Self> {{")?;
			writeln!(dest, "\t\tmatch object {{")?;
			writeln!(dest, "\t\t\tObject::{bare_ty}(obj) => Some(obj),")?;
			writeln!(dest, "\t\t\t_ => None,")?;
			writeln!(dest, "\t\t}}")?;
			writeln!(dest, "\t}}")?;
		}
		writeln!(dest, "}}")?;
	}
	writeln!(dest, "#[derive(Debug)]")?;
	writeln!(dest, "pub enum Object {{")?;
	for &(_, ty) in IMPL_TYPES {
		let bare_ty = ty.rsplit_once(':').map_or(ty, |(_, name)| name);
		writeln!(dest, "\t{bare_ty}({ty}),")?;
	}
	writeln!(dest, "}}")?;
	Ok(())
}

fn emit_interface(dest: &mut impl Write, iface: &Interface, impl_type: Option<&str>) -> Result<()> {
	if let Some(desc) = iface.desc {
		write_multiline(dest, "/// ", [desc.summary, desc.description])?;
	}

	// requests, as a trait of handlers
	let trait_name = RustName(iface.name);
	writeln!(dest, "pub mod {} {{", iface.name)?;
	writeln!(dest, "\tuse crate::client::SendHalf;")?;
	writeln!(dest, "\tuse crate::objects::{{Objects, OccupiedEntry, VacantEntry}};")?;
	writeln!(dest, "\tuse crate::protocol_types::{{Args, Fixed, FromArgs, Id, Fd}};")?;
	writeln!(dest, "\tuse super::Object;")?;
	writeln!(dest, "\tuse log::trace;")?;
	writeln!(dest, "\tuse std::io::{{self, ErrorKind, Result}};")?;
	writeln!(dest, "\t#[allow(clippy::too_many_arguments)]")?;
	writeln!(dest, "\tpub trait {trait_name}: Sized {{")?;
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
			write!(dest, "self, ")?;
		} else {
			write!(dest, "&mut self, ")?;
		}
		write!(dest, "client: &mut SendHalf<'_>, ")?;
		for arg in &req.args {
			write!(dest, "{}: {}, ", arg.name, RustArgType(arg.ty))?;
		}
		writeln!(dest, ") -> Result<()>;")?;
	}
	writeln!(dest, "\t}}")?;
	if let Some(impl_type) = impl_type {
		writeln!(dest, "\timpl {impl_type} where Self: {trait_name} {{")?;
		writeln!(dest, "\t\t#[allow(unused_mut, clippy::match_single_binding)]")?;
		writeln!(
			dest,
			"\t\tpub fn handle_request(objects: &mut Objects, client: &mut SendHalf<'_>, this_id: Id<Object>, opcode: \
			 u16, mut args: Args<'_>) -> Result<()> {{"
		)?;
		writeln!(dest, "\t\t\tmatch opcode {{")?;
		for (i, req) in iface.requests.iter().enumerate() {
			writeln!(dest, "\t\t\t\t{i} => {{")?;
			for arg in &req.args {
				writeln!(
					dest,
					"\t\t\t\t\ttrace!(\"parsing argument {}: {1} as {1:#} from {{args:?}}\");",
					arg.name,
					RustArgType(arg.ty)
				)?;
				writeln!(dest, "\t\t\t\t\tlet {} = <{:#}>::from_args(&mut args)?;", arg.name, RustArgType(arg.ty))?;
			}
			writeln!(dest, "\t\t\t\t\targs.finish()?;")?;
			writeln!(
				dest,
				"\t\t\t\t\tlet [this{args}] = objects.get_many_mut([this_id{args}])?;",
				args = IdArgs(&req.args)
			)?;
			writeln!(dest, "\t\t\t\t\tlet mut this = this.into_occupied()?.downcast::<Self>()?;")?;
			for arg in &req.args {
				match arg.ty {
					ArgType::Object { .. } => {
						writeln!(dest, "\t\t\t\t\tlet {name} = {name}.into_occupied()?.downcast()?;", name = arg.name)?
					},
					ArgType::NewId { .. } => {
						writeln!(dest, "\t\t\t\t\tlet {name} = {name}.into_vacant()?.downcast();", name = arg.name)?
					},
					_ => (),
				}
			}
			if req.kind == Some("destructor") {
				write!(dest, "\t\t\t\t\tthis.take().handle_{}(client, ", req.name)?;
			} else {
				write!(dest, "\t\t\t\t\tthis.handle_{}(client, ", req.name)?;
			}
			for arg in &req.args {
				write!(dest, "{}, ", arg.name)?;
			}
			writeln!(dest, ")")?;
			writeln!(dest, "\t\t\t\t}},")?;
		}
		// ignore unused_variables for arguments without suppressing the lint for the entire function
		writeln!(
			dest,
			"\t\t\t\t_ => {{ let _ = (objects, client, this_id, args); Err(ErrorKind::InvalidInput.into()) }},"
		)?;
		writeln!(dest, "\t\t\t}}")?;
		writeln!(dest, "\t\t}}")?;
		writeln!(dest, "\t}}")?;
	}

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
		writeln!(dest, "\t\tfn from_args(args: &mut Args<'a>) -> Result<Self> {{")?;
		writeln!(dest, "\t\t\tmatch u32::from_args(args)? {{")?;
		for ent in &en.entries {
			writeln!(dest, "\t\t\t\t{} => Ok(Self::{}),", ent.value, RustName(ent.name))?;
		}
		writeln!(dest, "\t\t\t\t_ => Err(io::Error::new(ErrorKind::InvalidInput, \"invalid {name}\")),")?;
		writeln!(dest, "\t\t\t}}")?; // match
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

/// Format a Wayland <arg> type ([`ArgType`]) as Rust code for the corresponding Rust type.
/// With the alternate flag (`{arg_type:#}`), format as the type that implements `FromArgs` for parsing an argument from
/// a message.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct RustArgType<'a>(ArgType<'a>);

impl RustArgType<'_> {
	fn emit_object(new_id: bool, iface: Option<&str>, nullable: bool, f: &mut Formatter<'_>) -> fmt::Result {
		if nullable {
			f.write_str("Option<")?;
		}
		if f.alternate() {
			// emit as FromArgs type
			f.write_str("Id<Object>")?;
		} else {
			let entry_type = if new_id { "Vacant" } else { "Occupied" };
			let iface = iface.and_then(impl_of).unwrap_or("Object");
			write!(f, "{entry_type}Entry<'_, {iface}>")?;
		}
		if nullable {
			f.write_str(">")?;
		}
		Ok(())
	}
}

impl Display for RustArgType<'_> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		match self.0 {
			ArgType::Int => f.write_str("i32"),
			ArgType::Uint => f.write_str("u32"),
			ArgType::Enum(wl_name) => RustName(wl_name).fmt(f),
			ArgType::Fixed => f.write_str("Fixed"),
			ArgType::String { nullable: false } => f.write_str("&str"),
			ArgType::String { nullable: true } => f.write_str("Option<&str>"),
			ArgType::Object { interface, nullable } => Self::emit_object(false, interface, nullable, f),
			ArgType::NewId { interface } => Self::emit_object(true, interface, false, f),
			ArgType::Array => f.write_str("&[Word]"),
			ArgType::Fd => f.write_str("Fd"),
		}
	}
}

#[derive(Copy, Clone, Debug)]
struct IdArgs<'a>(&'a [Arg<'a>]);

impl Display for IdArgs<'_> {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		for arg in self.0 {
			if matches!(arg.ty, ArgType::Object { .. } | ArgType::NewId { .. }) {
				f.write_str(", ")?;
				f.write_str(arg.name)?;
			}
		}
		Ok(())
	}
}
