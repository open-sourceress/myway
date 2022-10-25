use roxmltree::{Attribute, Document, Node, NodeType};
use std::{io::Result, num::NonZeroU32};

use crate::types::{Description, Interface, Protocol};

/// Extract and parse typed attributes from an element [`Node`].
macro_rules! attributes {
	($elem:ident; $($attr:ident : $ty:tt ),* $(,)?) => {
		$(let mut $attr: Option<&Attribute> = None;)*
		for attr in $elem.attributes() {
			match attr.name() {
				$(
					stringify!($attr) => {
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
				Some($attr) => Some(attributes!(@parse $elem, $attr, $ty)),
				None => None,
			};
			attributes!(@unwrap $elem, $attr, $ty);
		)*
	};

	(@parse $elem:ident, $attr:ident, Option<$innerty:ty>) => {
		attributes!(@parse $elem, $attr, $innerty)
	};
	(@parse $elem:ident, $attr:ident, str) => {
		$attr.value()
	};
	(@parse $elem:ident, $attr:ident, $ty:ty) => {
		match $attr.value().parse::<$ty>() {
			Ok(value) => value,
			Err(err) => bail!("attribute {:?} value {:?} (at {:?}) can't be parsed as a {}: {err:?}", $attr.name(), $attr.value(), $attr.value_range(), stringify!($ty)),
		}
	};
	(@unwrap $elem:ident, $attr:ident, Option<$ty:ty>) => {};
	(@unwrap $elem:ident, $attr:ident, $ty:ty) => {
		let $attr = match $attr {
			Some(attr) => attr,
			None => bail!("element <{:?}> (at {:?}) missing required attribute {:?}", $elem.tag_name(), $elem.range(), stringify!($attr)),
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

	while let Some(elem) = children.next()? {
		let _ = elem;
	}

	Ok(Interface { name, version, desc })
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
					None => bail!("expected text inside <description> element (at {:?})", node.range()),
				};
				Ok(Some(Description { summary, description }))
			},
			None => Ok(None),
		}
	}
}
