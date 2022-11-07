use crate::types::{Arg, ArgType, Enum, Interface, Message, Protocol};
use std::{
	fmt::{self, Display, Formatter, Write as _},
	io::{Result, Write},
};

/// Map of protocol interface types to their corresponding Rust implementation type.
static IMPL_TYPES: &[(&str, &str)] = &[
	("wl_display", "crate::object_impls::Display"),
	("wl_callback", "crate::object_impls::Callback"),
	("wl_registry", "crate::object_impls::Registry"),
	("wl_shm", "crate::object_impls::ShmGlobal"),
	("wl_shm_pool", "crate::object_impls::ShmPool"),
	("wl_buffer", "crate::object_impls::ShmBuffer"),
];

/// Find the Rust implementation type for a given protocol interface.
fn impl_of<'a, 'b>(iface: &'b str) -> Option<&'a str> {
	IMPL_TYPES.iter().find(|&&(ifa, _)| ifa == iface).map(|&(_, ty)| ty)
}

pub(crate) fn emit_protocol(protocol: &Protocol<'_>, dest: &mut impl Write) -> Result<()> {
	if let Some(c) = protocol.copyright {
		writeln!(dest, "// Copyright of the protocol specification:")?;
		write_multiline(dest, "// > ", [c])?;
	}
	writeln!(dest, "use crate::{{client::{{RecvMessage, SendHalf}}, object_map::{{Object, Objects}}}};")?;
	writeln!(dest, "use super::Id;")?;
	if let Some(desc) = protocol.desc {
		write_multiline(dest, "//! ", [desc.summary, desc.description])?;
	}
	for iface in &protocol.interfaces {
		emit_interface(dest, iface, impl_of(iface.name))?;
	}
	for &(_, ty) in IMPL_TYPES {
		let bare_ty = ty.rsplit_once(':').map_or(ty, |(_, name)| name);
		writeln!(dest, "impl Object for {ty} {{")?;
		writeln!(dest, "\tfn upcast(self) -> AnyObject {{")?;
		writeln!(dest, "\t\tAnyObject::{bare_ty}(self)")?;
		writeln!(dest, "\t}}")?;
		for (func, ref_sigil) in [("downcast", ""), ("downcast_ref", "&"), ("downcast_mut", "&mut ")] {
			writeln!(dest, "\tfn {func}(object: {ref_sigil}AnyObject) -> Option<{ref_sigil}Self> {{")?;
			writeln!(dest, "\t\tmatch object {{")?;
			writeln!(dest, "\t\t\tAnyObject::{bare_ty}(obj) => Some(obj),")?;
			writeln!(dest, "\t\t\t_ => None,")?;
			writeln!(dest, "\t\t}}")?;
			writeln!(dest, "\t}}")?;
		}
		writeln!(dest, "}}")?;
	}
	writeln!(dest, "#[derive(Debug)]")?;
	writeln!(dest, "pub enum AnyObject {{")?;
	for &(_, ty) in IMPL_TYPES {
		let bare_ty = ty.rsplit_once(':').map_or(ty, |(_, name)| name);
		writeln!(dest, "\t{bare_ty}({ty}),")?;
	}
	writeln!(dest, "}}")?;
	writeln!(dest, "impl AnyObject {{")?;
	writeln!(
		dest,
		"\tpub fn request_handler(&self) -> fn(&mut Objects, &mut SendHalf<'_>, RecvMessage<'_>) -> \
		 std::io::Result<()> {{"
	)?;
	writeln!(dest, "\t\tmatch self {{")?;
	for &(_, ty) in IMPL_TYPES {
		let variant = ty.rsplit_once(':').map_or(ty, |(_, name)| name);
		writeln!(dest, "\t\t\tSelf::{variant}(_) => {ty}::handle_request,")?;
	}
	writeln!(dest, "\t\t}}")?;
	writeln!(dest, "\t}}")?;
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
	writeln!(dest, "\tuse crate::client::{{RecvMessage, SendMessage, SendHalf}};")?;
	writeln!(dest, "\tuse crate::object_map::{{Objects, OccupiedEntry, VacantEntry}};")?;
	writeln!(dest, "\tuse crate::protocol::{{Word, Fd, Fixed, DecodeArg, Id, EncodeArg}};")?;
	writeln!(dest, "\tuse super::AnyObject;")?;
	writeln!(dest, "\tuse log::trace;")?;
	writeln!(dest, "\tuse std::{{io::{{self, ErrorKind, Result}}, os::unix::io::AsRawFd}};")?;
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
			write!(dest, "{}: {}, ", arg.name, RustArgType(arg.ty, TypePosition::Handler))?;
		}
		writeln!(dest, ") -> Result<()>;")?;
	}
	writeln!(dest, "\t}}")?;

	if let Some(impl_type) = impl_type {
		writeln!(dest, "\timpl {impl_type} where Self: {trait_name} {{")?;
		writeln!(dest, "\t\tpub const INTERFACE: &str = {:?};", iface.name)?;
		writeln!(dest, "\t\tpub const VERSION: u32 = {};", iface.version)?;
		emit_request_handler(dest, iface)?;
		for (opcode, ev) in iface.events.iter().enumerate() {
			writeln!(dest, "\t\t#[allow(unused_mut)]")?;
			write!(dest, "\t\tpub fn send_{}(", ev.name)?;
			if ev.kind == Some("destructor") {
				write!(dest, "self")?;
			} else {
				write!(dest, "&self")?;
			}
			write!(dest, ", self_id: Id<Self>, client: &mut SendHalf<'_>")?;
			for arg in &ev.args {
				write!(dest, ", {}: {}", arg.name, RustArgType(arg.ty, TypePosition::Event))?;
			}
			writeln!(dest, ") -> Result<()> {{")?;
			emit_log(dest, "\t\t\t", "event", ev)?;
			writeln!(dest, "\t\t\tlet (mut len, mut fds) = (0, 0);")?;
			for arg in &ev.args {
				writeln!(dest, "\t\t\tlen += {}.encoded_len();", arg.name)?;
				writeln!(dest, "\t\t\tfds += {}.is_fd() as usize;", arg.name)?;
			}
			writeln!(dest, "\t\t\tlet mut event = client.submit(self_id.cast(), {opcode}, len as usize, fds)?;")?;
			for arg in &ev.args {
				writeln!(
					dest,
					"\t\t\ttrace!(\"encoding argument {0}={{{0}:?}} (type: {1}) for event\");",
					arg.name,
					RustArgType(arg.ty, TypePosition::Event)
				)?;
				writeln!(dest, "\t\t\t{}.encode(&mut event);", arg.name)?;
			}
			writeln!(dest, "\t\t\tevent.finish();")?;
			writeln!(dest, "\t\t\tOk(())")?;
			writeln!(dest, "\t\t}}")?;
		}
		writeln!(dest, "\t}}")?;
	}

	for en in &iface.enums {
		emit_enum(dest, en)?;
	}

	writeln!(dest, "}}")?;
	Ok(())
}

/// Emit  `fn handle_request(..) -> Result<()>` for an interface implementation.
/// The function dispatches requests to the appropriate method by opcode.
fn emit_request_handler(dest: &mut impl Write, iface: &Interface<'_>) -> Result<()> {
	writeln!(dest, "\t\t#[allow(unused_mut, clippy::match_single_binding)]")?; // for interfaces with no requests
	writeln!(
		dest,
		"\t\tpub fn handle_request(objects: &mut Objects, client: &mut SendHalf<'_>, mut message: RecvMessage<'_>) -> \
		 Result<()> {{"
	)?;
	writeln!(dest, "\t\t\tlet self_id = message.object_id();")?;
	writeln!(dest, "\t\t\tmatch message.opcode() {{")?;
	for (i, req) in iface.requests.iter().enumerate() {
		writeln!(dest, "\t\t\t\t{i} => {{")?;
		for arg in &req.args {
			writeln!(
				dest,
				"\t\t\t\t\ttrace!(\"decoding argument {} (type: {}) from {{message:?}}\");",
				arg.name,
				RustArgType(arg.ty, TypePosition::Handler),
			)?;
			writeln!(
				dest,
				"\t\t\t\t\tlet {} = <{:#}>::decode_arg(&mut message)?;",
				arg.name,
				RustArgType(arg.ty, TypePosition::RawProtocol),
			)?;
		}
		writeln!(dest, "\t\t\t\t\tmessage.finish()?;")?;
		emit_log(dest, "\t\t\t\t\t", "request", req)?;

		writeln!(
			dest,
			"\t\t\t\t\tlet [this{args}] = objects.get_many_mut([self_id{args}])?;",
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
	writeln!(dest, "\t\t\t\t_ => {{")?;
	// ignore unused_variables for arguments without suppressing the lint for the entire function
	writeln!(dest, "\t\t\t\t\tlet _ = (objects, client, self_id);")?;
	writeln!(dest, "\t\t\t\t\tErr(io::Error::new(ErrorKind::InvalidInput, \"unknown request opcode {{opcode}}\"))")?;
	writeln!(dest, "\t\t\t\t}},")?; // match arm
	writeln!(dest, "\t\t\t}}")?; // match body
	writeln!(dest, "\t\t}}")?; // method body
	Ok(())
}

/// Emit code to log a message in WAYLAND_DEBUG-compatible format.
fn emit_log(dest: &mut impl Write, indent: &str, kind: &str, message: &Message) -> Result<()> {
	writeln!(dest, "{indent}#[allow(unused_mut)]")?; // messages with no args
	writeln!(
		dest,
		"{indent}if let Some(mut log) = crate::logging::log_{kind}(Self::INTERFACE, {:?}, self_id.into()) {{",
		message.name
	)?;
	for &Arg { name, ty, .. } in &message.args {
		match ty {
			ArgType::Uint | ArgType::Int | ArgType::Fixed | ArgType::String { nullable: false } => {
				writeln!(dest, "{indent}\tlog.arg_debug({name});")?
			},
			ArgType::Enum(_) => writeln!(dest, "{indent}\tlog.arg_debug({name} as u32);")?,
			ArgType::String { nullable: true } => {
				writeln!(dest, "{indent}\tmatch {name} {{")?;
				writeln!(dest, "{indent}\t\tSome(arg) => log.arg_debug(arg),")?;
				writeln!(dest, "{indent}\t\tNone => log.arg_nil(),")?;
				writeln!(dest, "{indent}\t}}")?;
			},
			ArgType::Object { interface, nullable: false } => {
				writeln!(dest, "{indent}\tlog.arg_object({interface:?}, {name}.into());")?
			},
			ArgType::Object { interface, nullable: true } => {
				writeln!(dest, "{indent}\tmatch {name} {{")?;
				writeln!(dest, "{indent}\t\tSome(id) => log.arg_object({interface:?}, id.into()),")?;
				writeln!(dest, "{indent}\t\tNone => log.arg_nil(),")?;
				writeln!(dest, "{indent}\t}}")?;
			},
			ArgType::NewId { interface } => writeln!(dest, "{indent}\tlog.arg_new_id({interface:?}, {name}.into());")?,
			ArgType::Array => writeln!(dest, "{indent}\tlog.arg_array({name});")?,
			ArgType::Fd => writeln!(dest, "{indent}\tlog.arg_fd(&{name});")?,
		}
	}
	writeln!(dest, "{indent}\tlog.finish();")?;
	writeln!(dest, "{indent}}}")?;
	Ok(())
}

fn emit_enum(dest: &mut impl Write, en: &Enum) -> Result<()> {
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

	writeln!(dest, "\timpl<'a> DecodeArg<'a> for {name} {{")?;
	writeln!(dest, "\t\tfn decode_arg(message: &mut RecvMessage<'a>) -> Result<Self> {{")?;
	writeln!(dest, "\t\t\tmatch u32::decode_arg(message)? {{")?;
	for ent in &en.entries {
		writeln!(dest, "\t\t\t\t{} => Ok(Self::{}),", ent.value, RustName(ent.name))?;
	}
	writeln!(dest, "\t\t\t\t_ => Err(io::Error::new(ErrorKind::InvalidInput, \"invalid {name}\")),")?;
	writeln!(dest, "\t\t\t}}")?; // match
	writeln!(dest, "\t\t}}")?; // fn
	writeln!(dest, "\t}}")?; // trait impl

	writeln!(dest, "\timpl EncodeArg for {name} {{")?;
	writeln!(dest, "\t\tfn encoded_len(&self) -> u16 {{")?;
	writeln!(dest, "\t\t\t1")?;
	writeln!(dest, "\t\t}}")?;
	writeln!(dest, "\t\tfn encode(&self, event: &mut SendMessage<'_>) {{")?;
	writeln!(dest, "\t\t\t(*self as u32).encode(event);")?;
	writeln!(dest, "\t\t}}")?;
	writeln!(dest, "\t}}")?;
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
/// With the alternate flag (`{arg_type:#}`), format as the type that implements `DecodeArg` for parsing an argument
/// from a message.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct RustArgType<'a>(ArgType<'a>, TypePosition);

impl RustArgType<'_> {
	fn emit_object(&self, new_id: bool, iface: Option<&str>, nullable: bool, f: &mut Formatter<'_>) -> fmt::Result {
		if nullable {
			f.write_str("Option<")?;
		}
		match self.1 {
			TypePosition::Handler => {
				let entry_type = if new_id { "Vacant" } else { "Occupied" };
				let iface = iface.and_then(impl_of).unwrap_or("AnyObject");
				write!(f, "{entry_type}Entry<'_, {iface}>")?;
			},
			TypePosition::Event => {
				let iface = iface.and_then(impl_of).unwrap_or("AnyObject");
				write!(f, "Id<{iface}>")?;
			},
			TypePosition::RawProtocol => {
				f.write_str("Id<AnyObject>")?;
			},
		}
		if nullable {
			f.write_str(">")?;
		}
		Ok(())
	}
}

/// Position in which a [`RustArgType`] is being emitted.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum TypePosition {
	/// As an argument type in a request handler: strongest typing, passed by-value.
	Handler,
	/// As an argument type in an event sender: strongest typing, passed by-reference.
	Event,
	/// As a type that implements `DecodeArg` for argument parsing.
	RawProtocol,
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
			ArgType::Object { interface, nullable } => self.emit_object(false, interface, nullable, f),
			ArgType::NewId { interface } => self.emit_object(true, interface, false, f),
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
