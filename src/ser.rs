/*!
The serializer, which turns any `Serialize` value into a tree of nodes, with the members of each object in the order they were given. The writer sorts them for canonical form, and `to_value` makes a `Value` of them.
*/

use crate::parse::{Kind, MAX_DEPTH, Member, Node, Object, Offset};
use crate::write::too_deep;
use crate::{Duration, Error, Instant};
use serde_core::ser::{self, Impossible, Serialize};
use std::borrow::Cow;
use std::fmt::Display;

/**
Widens an `f32` through its shortest decimal, so `0.1f32` becomes `0.1`, which reads back as the same `f32`, rather than `0.10000000149011612`.
*/
pub(crate) fn widen(value: f32) -> f64 {
	if !value.is_finite() {
		return f64::from(value);
	}

	zmij::Buffer::new()
		.format_finite(value)
		.parse()
		.expect("zmij writes a valid float")
}

/**
Turns a value into a node. `depth` is the number of collections around the value.
*/
#[derive(Clone, Copy)]
pub(crate) struct NodeSerializer {
	pub depth: usize,
}

impl NodeSerializer {
	fn nested(self) -> Result<Self, Error> {
		let depth = self.depth + 1;

		if depth > MAX_DEPTH {
			return Err(too_deep());
		}

		Ok(Self { depth })
	}
}

pub(crate) fn integer(value: impl TryInto<i64> + Display + Copy) -> Result<i64, Error> {
	value.try_into().map_err(|_| {
		Error::write(format!(
			"The integer {value} is outside the 64-bit range of a SOML int"
		))
	})
}

/**
A value as a one-member object, `{variant: value}`, the externally tagged form of an enum variant.
*/
fn tagged(variant: &str, value: Node<'static>) -> Result<Node<'static>, Error> {
	crate::write::check_representable(variant, "key")?;
	Ok(Node::from(Kind::Object(Object::new(vec![(
		Cow::Owned(variant.to_owned()),
		Member {
			key_offset: Offset::NONE,
			value,
		},
	)]))))
}

impl ser::Serializer for NodeSerializer {
	type Ok = Node<'static>;
	type Error = Error;
	type SerializeSeq = SeqSerializer;
	type SerializeTuple = SeqSerializer;
	type SerializeTupleStruct = SeqSerializer;
	type SerializeTupleVariant = SeqSerializer;
	type SerializeMap = MapSerializer;
	type SerializeStruct = MapSerializer;
	type SerializeStructVariant = MapSerializer;

	fn serialize_bool(self, value: bool) -> Result<Node<'static>, Error> {
		Ok(Node::from(Kind::Bool(value)))
	}

	fn serialize_i8(self, value: i8) -> Result<Node<'static>, Error> {
		Ok(Node::from(Kind::Int(value.into())))
	}

	fn serialize_i16(self, value: i16) -> Result<Node<'static>, Error> {
		Ok(Node::from(Kind::Int(value.into())))
	}

	fn serialize_i32(self, value: i32) -> Result<Node<'static>, Error> {
		Ok(Node::from(Kind::Int(value.into())))
	}

	fn serialize_i64(self, value: i64) -> Result<Node<'static>, Error> {
		Ok(Node::from(Kind::Int(value)))
	}

	fn serialize_i128(self, value: i128) -> Result<Node<'static>, Error> {
		integer(value).map(|value| Node::from(Kind::Int(value)))
	}

	fn serialize_u8(self, value: u8) -> Result<Node<'static>, Error> {
		Ok(Node::from(Kind::Int(value.into())))
	}

	fn serialize_u16(self, value: u16) -> Result<Node<'static>, Error> {
		Ok(Node::from(Kind::Int(value.into())))
	}

	fn serialize_u32(self, value: u32) -> Result<Node<'static>, Error> {
		Ok(Node::from(Kind::Int(value.into())))
	}

	fn serialize_u64(self, value: u64) -> Result<Node<'static>, Error> {
		integer(value).map(|value| Node::from(Kind::Int(value)))
	}

	fn serialize_u128(self, value: u128) -> Result<Node<'static>, Error> {
		integer(value).map(|value| Node::from(Kind::Int(value)))
	}

	fn serialize_f32(self, value: f32) -> Result<Node<'static>, Error> {
		self.serialize_f64(widen(value))
	}

	fn serialize_f64(self, value: f64) -> Result<Node<'static>, Error> {
		if value.is_nan() {
			return Err(Error::write("NaN is not a SOML value"));
		}

		// Zero has one value whatever its sign.
		Ok(Node::from(Kind::Float(if value == 0.0 {
			0.0
		} else {
			value
		})))
	}

	fn serialize_char(self, value: char) -> Result<Node<'static>, Error> {
		self.serialize_str(value.encode_utf8(&mut [0; 4]))
	}

	fn serialize_str(self, value: &str) -> Result<Node<'static>, Error> {
		crate::write::check_representable(value, "string")?;
		Ok(Node::from(Kind::String(Cow::Owned(value.to_owned()))))
	}

	fn serialize_bytes(self, value: &[u8]) -> Result<Node<'static>, Error> {
		self.nested()?;
		Ok(Node::from(Kind::Array(
			value
				.iter()
				.map(|&byte| Node::from(Kind::Int(byte.into())))
				.collect(),
		)))
	}

	fn serialize_none(self) -> Result<Node<'static>, Error> {
		Ok(Node::from(Kind::Null))
	}

	fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Node<'static>, Error> {
		value.serialize(self)
	}

	fn serialize_unit(self) -> Result<Node<'static>, Error> {
		Ok(Node::from(Kind::Null))
	}

	fn serialize_unit_struct(self, _name: &'static str) -> Result<Node<'static>, Error> {
		Ok(Node::from(Kind::Null))
	}

	fn serialize_unit_variant(
		self,
		_name: &'static str,
		_index: u32,
		variant: &'static str,
	) -> Result<Node<'static>, Error> {
		self.serialize_str(variant)
	}

	fn serialize_newtype_struct<T: ?Sized + Serialize>(
		self,
		name: &'static str,
		value: &T,
	) -> Result<Node<'static>, Error> {
		match name {
			crate::instant::TOKEN => match value.serialize(self)?.kind {
				Kind::String(text) => Instant::parse(&text)
					.map(|instant| Node::from(Kind::Instant(instant)))
					.map_err(Error::write),
				_ => Err(Error::write("An instant must serialize as its text")),
			},
			crate::duration::TOKEN => match value.serialize(self)?.kind {
				Kind::String(text) => Duration::parse(&text)
					.map(|duration| Node::from(Kind::Duration(duration)))
					.map_err(Error::write),
				_ => Err(Error::write("A duration must serialize as its text")),
			},
			_ => value.serialize(self),
		}
	}

	fn serialize_newtype_variant<T: ?Sized + Serialize>(
		self,
		_name: &'static str,
		_index: u32,
		variant: &'static str,
		value: &T,
	) -> Result<Node<'static>, Error> {
		let nested = self.nested()?;
		tagged(variant, value.serialize(nested)?)
	}

	fn serialize_seq(self, length: Option<usize>) -> Result<SeqSerializer, Error> {
		Ok(SeqSerializer {
			items: Vec::with_capacity(length.unwrap_or(0).min(4096)),
			serializer: self.nested()?,
			variant: None,
		})
	}

	fn serialize_tuple(self, length: usize) -> Result<SeqSerializer, Error> {
		self.serialize_seq(Some(length))
	}

	fn serialize_tuple_struct(
		self,
		_name: &'static str,
		length: usize,
	) -> Result<SeqSerializer, Error> {
		self.serialize_seq(Some(length))
	}

	fn serialize_tuple_variant(
		self,
		_name: &'static str,
		_index: u32,
		variant: &'static str,
		length: usize,
	) -> Result<SeqSerializer, Error> {
		Ok(SeqSerializer {
			variant: Some(variant),
			..self.nested()?.serialize_seq(Some(length))?
		})
	}

	fn serialize_map(self, _length: Option<usize>) -> Result<MapSerializer, Error> {
		Ok(MapSerializer {
			object: Object::default(),
			next_key: None,
			serializer: self.nested()?,
			variant: None,
			struct_name: None,
		})
	}

	fn serialize_struct(self, name: &'static str, _length: usize) -> Result<MapSerializer, Error> {
		// A `std::time::Duration` becomes a duration, not an object, so it is not a level of nesting. The writer checks the depth of what it writes again.
		let serializer = if name == "Duration" {
			Self {
				depth: self.depth + 1,
			}
		} else {
			self.nested()?
		};

		Ok(MapSerializer {
			object: Object::default(),
			next_key: None,
			serializer,
			variant: None,
			struct_name: Some(name),
		})
	}

	fn serialize_struct_variant(
		self,
		_name: &'static str,
		_index: u32,
		variant: &'static str,
		_length: usize,
	) -> Result<MapSerializer, Error> {
		Ok(MapSerializer {
			variant: Some(variant),
			..self.nested()?.serialize_map(None)?
		})
	}
}

pub(crate) struct SeqSerializer {
	items: Vec<Node<'static>>,
	serializer: NodeSerializer,
	variant: Option<&'static str>,
}

impl SeqSerializer {
	fn push<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
		self.items.push(value.serialize(self.serializer)?);
		Ok(())
	}

	fn finish(self) -> Result<Node<'static>, Error> {
		let array = Node::from(Kind::Array(self.items));

		match self.variant {
			Some(variant) => tagged(variant, array),
			None => Ok(array),
		}
	}
}

macro_rules! serialize_items {
	($($trait:ident::$method:ident),*) => {
		$(
			impl ser::$trait for SeqSerializer {
				type Ok = Node<'static>;
				type Error = Error;

				fn $method<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
					self.push(value)
				}

				fn end(self) -> Result<Node<'static>, Error> {
					self.finish()
				}
			}
		)*
	};
}

serialize_items!(
	SerializeSeq::serialize_element,
	SerializeTuple::serialize_element,
	SerializeTupleStruct::serialize_field,
	SerializeTupleVariant::serialize_field
);

pub(crate) struct MapSerializer {
	object: Object<'static>,
	next_key: Option<String>,
	serializer: NodeSerializer,
	variant: Option<&'static str>,
	struct_name: Option<&'static str>,
}

impl MapSerializer {
	fn insert(&mut self, key: String, value: Node<'static>) -> Result<(), Error> {
		// Every key, from a map or a field name, is checked here.
		crate::write::check_representable(&key, "key")?;

		if self.object.position(&key).is_some() {
			return Err(Error::write(format!("Duplicate key “{key}”")));
		}

		self.object.push(
			Cow::Owned(key),
			Member {
				key_offset: Offset::NONE,
				value,
			},
		);
		Ok(())
	}

	fn finish(self) -> Result<Node<'static>, Error> {
		if self.struct_name == Some("Duration") {
			if let Some(duration) = std_duration(&self.object)? {
				return Ok(Node::from(Kind::Duration(duration)));
			}

			// Not a `std::time::Duration` after all, so it is a level of nesting, which `serialize_struct` did not count.
			if self.serializer.depth > MAX_DEPTH {
				return Err(too_deep());
			}

			return Ok(Node::from(Kind::Object(self.object)));
		}

		let object = Node::from(Kind::Object(self.object));

		match self.variant {
			Some(variant) => tagged(variant, object),
			None => Ok(object),
		}
	}
}

/**
A `std::time::Duration` serializes as a struct named `Duration` with the fields `secs` and `nanos`, and is written as a SOML duration. serde gives no other type information, so this is recognized by shape, as ron recognizes `Range`. A user struct with the same name and fields gets the same treatment, and it still reads back.

Returns `None` when the object does not have that shape.
*/
fn std_duration(object: &Object<'_>) -> Result<Option<Duration>, Error> {
	let field = |key| {
		object
			.position(key)
			.map(|position| &object.members[position].1.value.kind)
	};

	let shape = match (object.members.len(), field("secs"), field("nanos")) {
		(2, Some(Kind::Int(seconds)), Some(Kind::Int(nanoseconds)))
			if *seconds >= 0 && (0..1_000_000_000).contains(nanoseconds) =>
		{
			Some((*seconds, *nanoseconds))
		}
		_ => None,
	};

	let Some((seconds, nanoseconds)) = shape else {
		return Ok(None);
	};

	let total = seconds
		.checked_mul(1_000_000_000)
		.and_then(|total| total.checked_add(nanoseconds))
		.ok_or_else(|| {
			Error::write(format!(
				"A duration of {seconds} seconds is outside the SOML range of about 292 years"
			))
		})?;
	Ok(Some(Duration::from_nanoseconds(total)))
}

impl ser::SerializeMap for MapSerializer {
	type Ok = Node<'static>;
	type Error = Error;

	fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Error> {
		self.next_key = Some(key.serialize(KeySerializer)?);
		Ok(())
	}

	fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
		let key = self
			.next_key
			.take()
			.expect("serde calls serialize_key before serialize_value");
		let value = value.serialize(self.serializer)?;
		self.insert(key, value)
	}

	fn end(self) -> Result<Node<'static>, Error> {
		self.finish()
	}
}

macro_rules! serialize_fields {
	($($trait:ident),*) => {
		$(
			impl ser::$trait for MapSerializer {
				type Ok = Node<'static>;
				type Error = Error;

				fn serialize_field<T: ?Sized + Serialize>(
					&mut self,
					key: &'static str,
					value: &T,
				) -> Result<(), Error> {
					let value = value.serialize(self.serializer)?;
					self.insert(key.to_owned(), value)
				}

				fn end(self) -> Result<Node<'static>, Error> {
					self.finish()
				}
			}
		)*
	};
}

serialize_fields!(SerializeStruct, SerializeStructVariant);

/**
Turns a map key into a string. A key can be a string, a char, a bool, an integer (written in decimal), or a unit enum variant.
*/
struct KeySerializer;

#[cold]
fn key_error(what: &str) -> Error {
	Error::write(format!(
		"A map key must be a string, a char, a bool, an integer, or a unit enum variant, not {what}"
	))
}

/**
A bool or an integer as a key, in decimal.
*/
macro_rules! key_as_text {
	($($method:ident: $type:ty),*) => {
		$(
			fn $method(self, value: $type) -> Result<String, Error> {
				Ok(value.to_string())
			}
		)*
	};
}

impl ser::Serializer for KeySerializer {
	type Ok = String;
	type Error = Error;
	type SerializeSeq = Impossible<String, Error>;
	type SerializeTuple = Impossible<String, Error>;
	type SerializeTupleStruct = Impossible<String, Error>;
	type SerializeTupleVariant = Impossible<String, Error>;
	type SerializeMap = Impossible<String, Error>;
	type SerializeStruct = Impossible<String, Error>;
	type SerializeStructVariant = Impossible<String, Error>;

	key_as_text!(
		serialize_bool: bool,
		serialize_i8: i8,
		serialize_i16: i16,
		serialize_i32: i32,
		serialize_i64: i64,
		serialize_i128: i128,
		serialize_u8: u8,
		serialize_u16: u16,
		serialize_u32: u32,
		serialize_u64: u64,
		serialize_u128: u128
	);

	fn serialize_f32(self, _value: f32) -> Result<String, Error> {
		Err(key_error("a float"))
	}

	fn serialize_f64(self, _value: f64) -> Result<String, Error> {
		Err(key_error("a float"))
	}

	fn serialize_char(self, value: char) -> Result<String, Error> {
		self.serialize_str(value.encode_utf8(&mut [0; 4]))
	}

	fn serialize_str(self, value: &str) -> Result<String, Error> {
		// `MapSerializer::insert` checks every key.
		Ok(value.to_owned())
	}

	fn serialize_bytes(self, _value: &[u8]) -> Result<String, Error> {
		Err(key_error("bytes"))
	}

	fn serialize_none(self) -> Result<String, Error> {
		Err(key_error("None"))
	}

	fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<String, Error> {
		value.serialize(self)
	}

	fn serialize_unit(self) -> Result<String, Error> {
		Err(key_error("()"))
	}

	fn serialize_unit_struct(self, name: &'static str) -> Result<String, Error> {
		Err(key_error(&format!("the unit struct {name}")))
	}

	fn serialize_unit_variant(
		self,
		_name: &'static str,
		_index: u32,
		variant: &'static str,
	) -> Result<String, Error> {
		Ok(variant.to_owned())
	}

	fn serialize_newtype_struct<T: ?Sized + Serialize>(
		self,
		name: &'static str,
		value: &T,
	) -> Result<String, Error> {
		match name {
			crate::instant::TOKEN => Err(key_error("an instant")),
			crate::duration::TOKEN => Err(key_error("a duration")),
			_ => value.serialize(self),
		}
	}

	fn serialize_newtype_variant<T: ?Sized + Serialize>(
		self,
		_name: &'static str,
		_index: u32,
		variant: &'static str,
		_value: &T,
	) -> Result<String, Error> {
		Err(key_error(&format!(
			"the enum variant {variant} with a value"
		)))
	}

	fn serialize_seq(self, _length: Option<usize>) -> Result<Self::SerializeSeq, Error> {
		Err(key_error("a sequence"))
	}

	fn serialize_tuple(self, _length: usize) -> Result<Self::SerializeTuple, Error> {
		Err(key_error("a tuple"))
	}

	fn serialize_tuple_struct(
		self,
		name: &'static str,
		_length: usize,
	) -> Result<Self::SerializeTupleStruct, Error> {
		Err(key_error(&format!("the tuple struct {name}")))
	}

	fn serialize_tuple_variant(
		self,
		_name: &'static str,
		_index: u32,
		variant: &'static str,
		_length: usize,
	) -> Result<Self::SerializeTupleVariant, Error> {
		Err(key_error(&format!(
			"the enum variant {variant} with a value"
		)))
	}

	fn serialize_map(self, _length: Option<usize>) -> Result<Self::SerializeMap, Error> {
		Err(key_error("a map"))
	}

	fn serialize_struct(
		self,
		name: &'static str,
		_length: usize,
	) -> Result<Self::SerializeStruct, Error> {
		Err(key_error(&format!("the struct {name}")))
	}

	fn serialize_struct_variant(
		self,
		_name: &'static str,
		_index: u32,
		variant: &'static str,
		_length: usize,
	) -> Result<Self::SerializeStructVariant, Error> {
		Err(key_error(&format!(
			"the enum variant {variant} with a value"
		)))
	}
}
