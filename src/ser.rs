/*!
The serializer, which turns any `Serialize` value into a tree of nodes, with the members of each object in the order they were given. The writer sorts them for canonical form, and `to_value` makes a `Value` of them. `to_string` writes the text without the tree, through `stream`. It uses the tree for a struct named `Duration`, which may be a `std::time::Duration`, for a value that it cannot write, to find the error, and for a value whose `Serialize` impl catches an error and goes on.
*/

pub(crate) mod stream;

use crate::parse::{Kind, MAX_DEPTH, Member, Node, Object};
use crate::{Duration, Error, Instant};
use core::fmt::NumBuffer;
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
	pub(crate) fn nested(self) -> Result<Self, Error> {
		Ok(Self {
			depth: nested_depth(self.depth)?,
		})
	}
}

/**
Checks and returns one more level of collection nesting, shared by serializers and borrowed value validation.
*/
pub(crate) fn nested_depth(depth: usize) -> Result<usize, Error> {
	if depth >= MAX_DEPTH {
		return Err(too_deep());
	}
	Ok(depth + 1)
}

/**
The capacity to reserve for the items or members a `Serialize` implementation says it has. It is capped, because the number is only a hint, and a wrong one must not reserve memory that is never used, or more than there is.
*/
pub(crate) fn capacity<T>(length: Option<usize>) -> usize {
	const BUDGET: usize = 16 * 1024;
	length
		.unwrap_or(0)
		.min(BUDGET / std::mem::size_of::<T>().max(1))
}

#[cold]
fn too_deep() -> Error {
	Error::write(format!(
		"The value is nested more than {MAX_DEPTH} levels deep, so no reader would accept the document"
	))
}

pub(crate) fn integer(value: impl TryInto<i64> + Display + Copy) -> Result<i64, Error> {
	value.try_into().map_err(|_| {
		Error::write(format!(
			"The integer {value} is outside the 64-bit range of a SOML int"
		))
	})
}

/**
A string, which is borrowed when it is a name in the type, such as a unit variant.
*/
fn string(text: Cow<'static, str>) -> Result<Node<'static>, Error> {
	crate::write::check_representable(&text, "string")?;
	Ok(Node::from(Kind::String(text)))
}

/**
A value of an enum variant as a one-member object, `{variant: value}`, the externally tagged form. Without a variant, the value as it is.
*/
fn tagged(variant: Option<&'static str>, value: Node<'static>) -> Result<Node<'static>, Error> {
	let Some(variant) = variant else {
		return Ok(value);
	};

	crate::write::check_representable(variant, "key")?;
	Ok(Node::from(Kind::Object(Object::new(vec![(
		Cow::Borrowed(variant),
		Member::new(value),
	)]))))
}

/**
Integer types, which are an error only outside the range of a SOML int.
*/
macro_rules! serialize_integers {
	($($method:ident: $type:ty),*) => {
		$(
			fn $method(self, value: $type) -> Result<Self::Ok, Error> {
				self.serialize_i64(integer(value)?)
			}
		)*
	};
}

pub(crate) use serialize_integers;

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

	serialize_integers!(
		serialize_i8: i8,
		serialize_i16: i16,
		serialize_i32: i32,
		serialize_i128: i128,
		serialize_u8: u8,
		serialize_u16: u16,
		serialize_u32: u32,
		serialize_u64: u64,
		serialize_u128: u128
	);

	fn serialize_i64(self, value: i64) -> Result<Node<'static>, Error> {
		Ok(Kind::Int(value).into())
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
		Ok(Kind::String(Cow::Owned(value.to_owned())).into())
	}

	fn collect_str<T: ?Sized + Display>(self, value: &T) -> Result<Node<'static>, Error> {
		string(Cow::Owned(value.to_string()))
	}

	fn serialize_bytes(self, value: &[u8]) -> Result<Node<'static>, Error> {
		// Only the depth check is needed, because the items are ints, which nest nothing.
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
		self.serialize_none()
	}

	fn serialize_unit_struct(self, _name: &'static str) -> Result<Node<'static>, Error> {
		self.serialize_none()
	}

	fn serialize_unit_variant(
		self,
		_name: &'static str,
		_index: u32,
		variant: &'static str,
	) -> Result<Node<'static>, Error> {
		string(Cow::Borrowed(variant))
	}

	fn serialize_newtype_struct<T: ?Sized + Serialize>(
		self,
		name: &'static str,
		value: &T,
	) -> Result<Node<'static>, Error> {
		// `Instant` and `Duration` give their canonical text under a private name. Other formats write that text as a string, and this one parses it back into the native value.
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
		tagged(Some(variant), value.serialize(nested)?)
	}

	fn serialize_seq(self, length: Option<usize>) -> Result<SeqSerializer, Error> {
		Ok(SeqSerializer {
			serializer: self.nested()?,
			items: Vec::with_capacity(capacity::<Node<'_>>(length)),
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
		// The variant's one-member object and the array in it are two levels: one here, and one in `serialize_seq`.
		Ok(SeqSerializer {
			variant: Some(variant),
			..self.nested()?.serialize_seq(Some(length))?
		})
	}

	fn serialize_map(self, length: Option<usize>) -> Result<MapSerializer, Error> {
		Ok(MapSerializer::new(self.nested()?, length, None))
	}

	fn serialize_struct(self, name: &'static str, length: usize) -> Result<MapSerializer, Error> {
		// A `std::time::Duration` becomes a duration, not an object, so it is not a level of nesting. `MapSerializer::finish` checks the depth of a struct that turns out not to be one.
		let serializer = if name == "Duration" {
			Self {
				depth: self.depth + 1,
			}
		} else {
			self.nested()?
		};

		Ok(MapSerializer::new(serializer, Some(length), Some(name)))
	}

	fn serialize_struct_variant(
		self,
		_name: &'static str,
		_index: u32,
		variant: &'static str,
		length: usize,
	) -> Result<MapSerializer, Error> {
		// The variant's one-member object and the object in it are two levels: one here, and one in `serialize_map`.
		Ok(MapSerializer {
			variant: Some(variant),
			..self.nested()?.serialize_map(Some(length))?
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
		tagged(self.variant, Node::from(Kind::Array(self.items)))
	}
}

/**
The same sequence operation for each serde collection trait, with the type's own finish operation.
*/
macro_rules! serialize_items {
	($serializer:ty, $output:ty, $this:ident => $finish:block; $($trait:ident::$method:ident),*) => {
		$(
			impl ser::$trait for $serializer {
				type Ok = $output;
				type Error = Error;

				fn $method<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
					self.push(value)
				}

				fn end($this) -> Result<$output, Error> $finish
			}
		)*
	};
}

pub(crate) use serialize_items;

serialize_items!(SeqSerializer, Node<'static>, self => { self.finish() };
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
	/**
	Starts a map or struct with capped hinted storage and no pending key or variant.
	*/
	fn new(
		serializer: NodeSerializer,
		length: Option<usize>,
		struct_name: Option<&'static str>,
	) -> Self {
		Self {
			object: Object::new(Vec::with_capacity(capacity::<(Cow<'_, str>, Member<'_>)>(
				length,
			))),
			next_key: None,
			serializer,
			variant: None,
			struct_name,
		}
	}

	/**
	Writes a struct field with its borrowed static key.
	*/
	fn member<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Result<(), Error> {
		let value = value.serialize(self.serializer)?;
		self.insert(Cow::Borrowed(key), value)
	}
	fn insert(&mut self, key: Cow<'static, str>, value: Node<'static>) -> Result<(), Error> {
		// Every key, from a map or a field name, is checked here.
		crate::write::check_representable(&key, "key")?;

		// A map's keys are unique in Rust, but `#[serde(flatten)]` or a custom `Serialize` can give the same key twice.
		if self.object.position(&key).is_some() {
			return Err(Error::write(format!(
				"Duplicate key “{}”",
				crate::abbreviate(&key, 40)
			)));
		}

		self.object.push(key, Member::new(value));
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
		}

		tagged(self.variant, Node::from(Kind::Object(self.object)))
	}
}

/**
A `std::time::Duration` serializes as a struct named `Duration` with the fields `secs` and `nanos`, and is written as a SOML duration. serde gives no other type information, so this is recognized by shape, as ron recognizes `Range`. A user struct with the same name and fields gets the same treatment, and it still reads back.

Returns `None` when the object does not have that shape.
*/
fn std_duration(object: &Object<'_>) -> Result<Option<Duration>, Error> {
	let [(first_key, first), (second_key, second)] = object.members.as_slice() else {
		return Ok(None);
	};
	let (seconds, nanoseconds) = match (
		first_key.as_ref(),
		&first.value.kind,
		second_key.as_ref(),
		&second.value.kind,
	) {
		("secs", Kind::Int(seconds), "nanos", Kind::Int(nanoseconds))
		| ("nanos", Kind::Int(nanoseconds), "secs", Kind::Int(seconds))
			if *seconds >= 0 && (0..1_000_000_000).contains(nanoseconds) =>
		{
			(*seconds, *nanoseconds)
		}
		_ => return Ok(None),
	};

	// Note: Not using carrying_mul here because it needs an extra high-word check and signed conversion.
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
		let mut text = String::new();
		key.serialize(KeySerializer(&mut text))?;
		self.next_key = Some(text);
		Ok(())
	}

	fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
		let key = self
			.next_key
			.take()
			.expect("serde calls serialize_key before serialize_value");
		let value = value.serialize(self.serializer)?;
		self.insert(Cow::Owned(key), value)
	}

	fn serialize_entry<K: ?Sized + Serialize, V: ?Sized + Serialize>(
		&mut self,
		key: &K,
		value: &V,
	) -> Result<(), Error> {
		let mut text = String::new();
		key.serialize(KeySerializer(&mut text))?;
		let value = value.serialize(self.serializer)?;
		self.insert(Cow::Owned(text), value)
	}

	fn end(self) -> Result<Node<'static>, Error> {
		self.finish()
	}
}

/**
Struct fields share their owning type's member operation and finish contract.
*/
macro_rules! serialize_fields {
	($serializer:ty, $output:ty, $this:ident => $finish:block; $($trait:ident),*) => {
		$(
			impl ser::$trait for $serializer {
				type Ok = $output;
				type Error = Error;

				fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Result<(), Error> {
					self.member(key, value)
				}

				fn end($this) -> Result<$output, Error> $finish
			}
		)*
	};
}

pub(crate) use serialize_fields;

serialize_fields!(MapSerializer, Node<'static>, self => { self.finish() }; SerializeStruct, SerializeStructVariant);

/**
Turns a map key into a string, which it adds to the string it holds. A key can be a string, a char, a bool, an integer (written in decimal), or a unit enum variant.
*/
struct KeySerializer<'a>(&'a mut String);

#[cold]
fn key_error(what: &str) -> Error {
	Error::write(format!(
		"A map key must be a string, a char, a bool, an integer, or a unit enum variant, not {what}"
	))
}

/**
An integer as a key, in decimal.
*/
macro_rules! key_as_text {
	($($method:ident: $type:ty),*) => {
		$(
			fn $method(self, value: $type) -> Result<(), Error> {
				self.0.push_str(value.format_into(&mut NumBuffer::new()));
				Ok(())
			}
		)*
	};
}

/**
Small integer keys share the decimal writer of their signed or unsigned 64-bit type.
*/
macro_rules! serialize_small_keys {
	($($method:ident: $type:ty => $wide:ident),*) => {
		$(
			fn $method(self, value: $type) -> Result<(), Error> {
				self.$wide(value.into())
			}
		)*
	};
}

impl ser::Serializer for KeySerializer<'_> {
	type Ok = ();
	type Error = Error;
	type SerializeSeq = Impossible<(), Error>;
	type SerializeTuple = Impossible<(), Error>;
	type SerializeTupleStruct = Impossible<(), Error>;
	type SerializeTupleVariant = Impossible<(), Error>;
	type SerializeMap = Impossible<(), Error>;
	type SerializeStruct = Impossible<(), Error>;
	type SerializeStructVariant = Impossible<(), Error>;

	key_as_text!(serialize_i128: i128, serialize_u128: u128);

	fn serialize_bool(self, value: bool) -> Result<(), Error> {
		self.0.push_str(if value { "true" } else { "false" });
		Ok(())
	}

	serialize_small_keys! {
		serialize_i8: i8 => serialize_i64,
		serialize_i16: i16 => serialize_i64,
		serialize_i32: i32 => serialize_i64,
		serialize_u8: u8 => serialize_u64,
		serialize_u16: u16 => serialize_u64,
		serialize_u32: u32 => serialize_u64
	}

	fn serialize_i64(self, value: i64) -> Result<(), Error> {
		crate::write::write_int(self.0, value);
		Ok(())
	}

	fn serialize_u64(self, value: u64) -> Result<(), Error> {
		self.0.push_str(value.format_into(&mut NumBuffer::new()));
		Ok(())
	}

	fn serialize_f32(self, _value: f32) -> Result<(), Error> {
		Err(key_error("a float"))
	}

	fn serialize_f64(self, _value: f64) -> Result<(), Error> {
		Err(key_error("a float"))
	}

	fn serialize_char(self, value: char) -> Result<(), Error> {
		self.serialize_str(value.encode_utf8(&mut [0; 4]))
	}

	fn serialize_str(self, value: &str) -> Result<(), Error> {
		// The map serializers check every key.
		self.0.push_str(value);
		Ok(())
	}

	// Without the string that the default makes, which matters for the text of each instant and duration.
	fn collect_str<T: ?Sized + Display>(self, value: &T) -> Result<(), Error> {
		crate::write::write_display(self.0, value);
		Ok(())
	}

	fn serialize_bytes(self, _value: &[u8]) -> Result<(), Error> {
		Err(key_error("bytes"))
	}

	fn serialize_none(self) -> Result<(), Error> {
		Err(key_error("None"))
	}

	fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<(), Error> {
		value.serialize(self)
	}

	fn serialize_unit(self) -> Result<(), Error> {
		Err(key_error("()"))
	}

	fn serialize_unit_struct(self, name: &'static str) -> Result<(), Error> {
		Err(key_error(&format!("the unit struct {name}")))
	}

	fn serialize_unit_variant(
		self,
		_name: &'static str,
		_index: u32,
		variant: &'static str,
	) -> Result<(), Error> {
		self.serialize_str(variant)
	}

	fn serialize_newtype_struct<T: ?Sized + Serialize>(
		self,
		name: &'static str,
		value: &T,
	) -> Result<(), Error> {
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
	) -> Result<(), Error> {
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

#[cfg(test)]
mod tests {
	use super::*;
	use serde_core::ser::{SerializeMap, SerializeSeq, Serializer};

	#[test]
	fn false_hints_reserve_at_most_sixteen_kibibytes_per_collection() {
		let sequence = NodeSerializer { depth: 0 }
			.serialize_seq(Some(usize::MAX))
			.unwrap();
		let Kind::Array(items) = sequence.end().unwrap().kind else {
			panic!("expected an array");
		};
		assert!(items.capacity() * std::mem::size_of::<Node<'_>>() <= 16 * 1024);
		let map = NodeSerializer { depth: 0 }
			.serialize_map(Some(usize::MAX))
			.unwrap();
		let Kind::Object(object) = map.end().unwrap().kind else {
			panic!("expected an object");
		};
		assert!(
			object.members.capacity() * std::mem::size_of::<(Cow<'_, str>, Member<'_>)>()
				<= 16 * 1024
		);
	}
}
