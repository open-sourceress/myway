use crate::types::{ArgType, Description, Enum, Entry, Arg, Interface, Message, Protocol};
use roxmltree::{Attribute, Document, Node, NodeType};
use std::{io::Result, num::NonZeroU32};

/// Extract and parse typed attributes from an element [`Node`].
macro_rules! attributes {
	($elem:ident; $($attr:ident : $ty:tt $(<$gty:tt>)? ),* $(,)?) => {
		$(let mut $attr: Option<&Attribute> = None;)*
		for attr in $elem.attributes() {
			match attr.name() {
				$(
					attributes!(@stringify $attr) => {
						if let Some(first) = $attr {
							bail!("element <{:?}> (at {:?}) has duplicate attribute {:?}: first at {:?}, second at {:?}", $elem.tag_name(), $elem.range(), stringify!($attr), first.range(), attr.range());
						}
						$attr = Some(attr);
					},
				)*
				_ => bail!("element <{:?}> (at {:?}) has unknown attribute {:?} (at {:?})", $elem.tag_name(), $elem.range(), attr.name(), attr.range()),
			}
		}
		$(
			let $attr = match $attr {
				Some($attr) => Some(attributes!(@parse $elem, $attr, $ty $(<$gty>)?)),
				None => None,
			};
			attributes!(@unwrap $elem, $attr, $ty $(<$gty>)?);
		)*
	};

	(@stringify $attr:tt) => { attributes!(@stringify_inner $attr) };
	(@stringify_inner r#type) => { "type" };
	(@stringify_inner r#enum) => { "enum" };
	(@stringify_inner allow_null) => { "allow-null" };
	(@stringify_inner $attr:tt) => { stringify!($attr) };
	(@parse $elem:ident, $attr:ident, Option<$gty:tt>) => {
		attributes!(@parse $elem, $attr, $gty)
	};
	(@parse $elem:ident, $attr:ident, str) => {
		$attr.value()
	};
	(@parse $elem:ident, $attr:ident, $ty:ty) => {
		match $attr.value().parse::<$ty>() {
			Ok(value) => value,
			Err(err) => bail!("attribute {:?} value {:?} (at {:?}) can't be parsed as a {}: {err:?}", $attr.name(), $attr.value(), $attr.value_range(), attributes!(@stringify $ty)),
		}
	};
	(@unwrap $elem:ident, $attr:ident, Option<$ty:ty>) => {};
	(@unwrap $elem:ident, $attr:ident, $ty:ty) => {
		let $attr = match $attr {
			Some(attr) => attr,
			None => bail!("element <{:?}> (at {:?}) missing required attribute {:?}", $elem.tag_name(), $elem.range(), attributes!(@stringify $attr)),
		};
	}
}

pub(crate) fn build_protocol<'doc>(schema: &'doc Document<'_>) -> Result<Protocol<'doc>> {
	let schema = schema.root();
	ensure!(schema.node_type() == NodeType::Root, "expected Root node, found {:?}", schema.node_type());
	let mut children = Children::of(schema);
	let proto = match (children.next_assert("protocol")?, children.next_assert("protocol")?) {
		(None, _) => bail!("root has no children, expected 1"),
		(Some(elem), None) => elem,
		(Some(_), Some(_)) => bail!("root has multiple children, expected 1"),
	};
	attributes![proto; name: str];

	let mut children = Children::of(proto);
	let copyright = children.copyright()?;
	let desc = children.description()?;
	let mut interfaces = Vec::new();
	while let Some(node) = children.next_assert("interface")? {
		interfaces.push(build_interface(node)?);
	}

	Ok(Protocol { name, interfaces, copyright, desc })
}

fn build_interface<'doc>(node: Node<'doc, '_>) -> Result<Interface<'doc>> {
	attributes![node; name: str, version: NonZeroU32];
	let mut children = Children::of(node);
	let desc = children.description()?;

	let mut requests = Vec::new();
	let mut events = Vec::new();
	let mut enums = Vec::new();
	while let Some(elem) = children.next()? {
		match elem.tag_name().name() {
			"request" => requests.push(build_message(elem)?),
			"event" => events.push(build_message(elem)?),
			"enum" => enums.push(build_enum(elem)?),
			unknown => bail!(
				"expected <request>, <event>, or <enum> element in <interface> (at {:?}), found <{unknown}> (at {:?})",
				node.range(),
				elem.range()
			),
		}
	}

	Ok(Interface { name, version, desc, requests, events, enums })
}

fn build_message<'doc>(node: Node<'doc, '_>) -> Result<Message<'doc>> {
	attributes![node; name: str, r#type: Option<str>, since: Option<NonZeroU32>];
	let mut children = Children::of(node);
	let desc = children.description()?;
	let mut args = Vec::new();
	while let Some(elem) = children.next_assert("arg")? {
		let arg = build_arg(elem)?;
		// This mimics the behavior of wayland-scanner and matches the signature of requests marshalled by wayland-client.h
		if matches!(arg.ty, ArgType::NewId { interface: None }) {
			args.push(Arg {
				name: "interface",
				ty: ArgType::String { nullable: false },
				summary: Some("requested interface to bind the object as (e.g. `\"wl_seat\\0\"`)"),
			});
			args.push(Arg {
				name: "version",
				ty: ArgType::String { nullable: false },
				summary: Some("version of the requested interface to bind as"),
			});
		}
		args.push(build_arg(elem)?);
	}
	Ok(Message { name, kind: r#type, since, desc, args })
}

fn build_arg<'doc>(node: Node<'doc, '_>) -> Result<Arg<'doc>> {
	attributes![node; name: str, r#type: str, summary: Option<str>, interface: Option<str>, allow_null: Option<bool>, r#enum: Option<str>];

	let ty = match (r#type, interface, allow_null.unwrap_or_default(), r#enum) {
		("int", None, false, None) => ArgType::Int,
		// <arg type="int" enum="wl_output.transform" /> exists in a few places for unknown reasons
		("int", None, false, Some(en)) => ArgType::Uint { r#enum: Some(en) },
		("uint", None, false, en) => ArgType::Uint { r#enum: en },
		("fixed", None, false, None) => ArgType::Fixed,
		("string", None, nullable, None) => ArgType::String { nullable },
		("object", interface, nullable, None) => ArgType::Object { interface, nullable },
		("new_id", interface, false, None) => ArgType::NewId { interface },
		("array", None, false, None) => ArgType::Array,
		("fd", None, false, None) => ArgType::Fd,
		(ty, inf, null, en) => bail!("invalid combination of type attributes for <arg> at {:?}: type={ty:?}, interface={inf:?}, nullable={null:?}, enum={en:?}", node.range()),
	};
	Ok(Arg { name, ty, summary })
}

fn build_enum<'doc>(node: Node<'doc, '_>) -> Result<Enum<'doc>> {
	attributes![node; name: str, since: Option<NonZeroU32>, bitfield: Option<bool>];
	let mut children = Children::of(node);
	let desc = children.description()?;
	let mut entries = Vec::new();
	while let Some(elem) = children.next_assert("entry")? {
		entries.push(build_entry(elem)?);
	}
	Ok(Enum { name, since, bitfield: bitfield.unwrap_or_default(), desc, entries })
}

fn build_entry<'doc>(node: Node<'doc, '_>) -> Result<Entry<'doc>> {
	attributes![node; name: str, value: str, summary: Option<str>, since: Option<NonZeroU32>];
	let (value_res, value_is_hex) = match value.strip_prefix("0x") {
		Some(hex) => (u32::from_str_radix(hex, 16), true),
		None => (value.parse(), false),
	};
	let value = match value_res {
		Ok(n) => n,
		Err(err) => bail!("attribute \"value\" value {value:?} can't be parsed as a u32: {err:?}"),
	};
	Ok(Entry { name, value, value_is_hex, summary, since })
}

#[derive(Debug)]
struct Children<'a, 'i> {
	node: Node<'a, 'i>,
	iter: roxmltree::Children<'a, 'i>,
	peek_slot: Option<Node<'a, 'i>>,
}

impl<'a, 'i> Children<'a, 'i> {
	fn of(node: Node<'a, 'i>) -> Self {
		Self { node, iter: node.children(), peek_slot: None }
	}

	fn next(&mut self) -> Result<Option<Node<'a, 'i>>> {
		if let Some(node) = self.peek_slot.take() {
			return Ok(Some(node));
		}
		loop {
			let child = match self.iter.next() {
				Some(n) => n,
				None => break Ok(None),
			};
			match child.node_type() {
				NodeType::Element => break Ok(Some(child)),
				NodeType::Comment => continue,
				NodeType::Text => {
					let text = child.text().unwrap_or_default();
					if !text.chars().all(char::is_whitespace) {
						bail!(
							"expected Element node in <{}> (at {:?}), found non-whitespace text {text:?} (at {:?})",
							self.node.tag_name().name(),
							self.node.range(),
							child.range()
						);
					}
				},
				other => bail!(
					"expected Element node in <{}> (at {:?}), found {other:?} (at {:?})",
					self.node.tag_name().name(),
					self.node.range(),
					child.range()
				),
			}
		}
	}

	fn next_if(&mut self, tag: &str) -> Result<Option<Node<'a, 'i>>> {
		let next = match self.next() {
			Ok(Some(n)) => n,
			err => return err,
		};
		if next.tag_name() == tag.into() {
			Ok(Some(next))
		} else {
			self.peek_slot = Some(next);
			Ok(None)
		}
	}

	fn next_assert(&mut self, tag: &str) -> Result<Option<Node<'a, 'i>>> {
		let next = match self.next() {
			Ok(Some(n)) => n,
			err => return err,
		};
		ensure!(
			next.tag_name() == tag.into(),
			"expected <{tag}> element in <{}> (at {:?}), found <{}> (at {:?})",
			self.node.tag_name().name(),
			self.node.range(),
			next.tag_name().name(),
			next.range()
		);
		Ok(Some(next))
	}

	fn copyright(&mut self) -> Result<Option<&'a str>> {
		match self.next_if("copyright")? {
			Some(node) => match node.text() {
				Some(t) => Ok(Some(t.trim())),
				None => bail!("expected text inside <copyright> element (at {:?})", node.range()),
			},
			None => Ok(None),
		}
	}

	fn description(&mut self) -> Result<Option<Description<'a>>> {
		match self.next_if("description")? {
			Some(node) => {
				attributes![node; summary: str];
				let description = match node.text() {
					Some(t) => t.trim(),
					// None => bail!("expected text inside <description> element (at {:?})", node.range()),
					None => "",
				};
				Ok(Some(Description { summary, description }))
			},
			None => Ok(None),
		}
	}
}
