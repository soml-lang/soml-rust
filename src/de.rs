/*!
The deserializer. It reads from the parsed tree, in which every value and key has its offset, so an error says where in the document it is. `from_value` converts a `Value` into the same tree without offsets, so there is one deserializer for both.
*/

use crate::parse::{Kind, Member, Node, Object as NodeObject, Offset};
use crate::{Error, Value};
use serde_core::de::value::{BorrowedStrDeserializer, MapDeserializer, StringDeserializer};
use serde_core::de::{
	self, DeserializeSeed, Deserializer, EnumAccess, Expected, IntoDeserializer, MapAccess,
	SeqAccess, Unexpected, VariantAccess, Visitor,
};
use serde_core::forward_to_deserialize_any;
use std::borrow::Cow;

pub(crate) struct NodeDeserializer<'de> {
	pub node: Node<'de>,
	/**
	The document, for the line and column of an error. `None` for a value that did not come from a document.
	*/
	pub source: Option<&'de str>,
}

fn unexpected<'a>(kind: &'a Kind<'_>) -> Unexpected<'a> {
	match kind {
		Kind::Null => Unexpected::Unit,
		Kind::Bool(value) => Unexpected::Bool(*value),
		Kind::Int(value) => Unexpected::Signed(*value),
		Kind::Float(value) => Unexpected::Float(*value),
		Kind::String(value) => Unexpected::Str(value),
		Kind::Instant(_) => Unexpected::Other("an instant"),
		Kind::Duration(_) => Unexpected::Other("a duration"),
		Kind::Array(_) => Unexpected::Seq,
		Kind::Object(_) => Unexpected::Map,
	}
}

impl<'de> NodeDeserializer<'de> {
	/**
	Runs a visit and gives an error without a position the position of this value.
	*/
	fn located<T>(self, visit: impl FnOnce(Self) -> Result<T, Error>) -> Result<T, Error> {
		let source = self.source;
		let offset = self.node.offset;
		visit(self).map_err(|error| error.or_at(source, offset.get()))
	}

	fn invalid_type(&self, expected: &dyn Expected) -> Error {
		de::Error::invalid_type(unexpected(&self.node.kind), expected)
	}

	fn visit_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		let source = self.source;

		match self.node.kind {
			Kind::Null => visitor.visit_unit(),
			Kind::Bool(value) => visitor.visit_bool(value),
			Kind::Int(value) => visitor.visit_i64(value),
			Kind::Float(value) => visitor.visit_f64(value),
			Kind::String(Cow::Borrowed(value)) => visitor.visit_borrowed_str(value),
			Kind::String(Cow::Owned(value)) => visitor.visit_string(value),
			// serde has no instant or duration type, so these are given as their canonical text. That is what jiff, chrono, and humantime types read, and what `serde_json::Value` can hold.
			Kind::Instant(value) => visitor.visit_string(value.to_string()),
			Kind::Duration(value) => visitor.visit_string(value.to_string()),
			Kind::Array(items) => {
				let count = items.len();
				let mut sequence = SeqDeserializer {
					items: items.into_iter(),
					source,
				};

				let value = visitor.visit_seq(&mut sequence)?;

				// A visitor that stops early, such as a tuple's, would otherwise drop the rest silently.
				if sequence.items.len() > 0 {
					return Err(de::Error::invalid_length(
						count,
						&"fewer items in the array",
					));
				}

				Ok(value)
			}
			Kind::Object(object) => {
				let count = object.members.len();
				let mut map = ObjectDeserializer::new(object, source);
				let value = visitor.visit_map(&mut map)?;

				if map.members.len() > 0 {
					return Err(de::Error::invalid_length(
						count,
						&"fewer members in the object",
					));
				}

				Ok(value)
			}
		}
	}

	/**
	Reads a `Value`, which keeps an instant and a duration as themselves.
	*/
	fn visit_value<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		match self.node.kind {
			Kind::Instant(value) => visitor.visit_enum(TokenEnum {
				name: crate::instant::TOKEN,
				text: value.to_string(),
			}),
			Kind::Duration(value) => visitor.visit_enum(TokenEnum {
				name: crate::duration::TOKEN,
				text: value.to_string(),
			}),
			_ => self.visit_any(visitor),
		}
	}

	fn visit_float<V: Visitor<'de>>(self, visitor: V, is_f32: bool) -> Result<V::Value, Error> {
		match self.node.kind {
			// An int reads into a float only when the float holds it exactly.
			Kind::Int(value) => {
				let float = value as f64;
				let read_back = if is_f32 {
					(value as f32) as i128
				} else {
					float as i128
				};

				if read_back != i128::from(value) {
					return Err(Error::data(format!(
						"The int {value} cannot be converted to a float exactly"
					)));
				}

				visitor.visit_f64(float)
			}
			_ => self.visit_if(
				|kind| !matches!(kind, Kind::Instant(_) | Kind::Duration(_)),
				visitor,
			),
		}
	}

	/**
	Visits the value when it is of a kind the caller accepts, and gives a type error otherwise.
	*/
	fn visit_if<V: Visitor<'de>>(
		self,
		is_accepted: fn(&Kind<'_>) -> bool,
		visitor: V,
	) -> Result<V::Value, Error> {
		if is_accepted(&self.node.kind) {
			self.visit_any(visitor)
		} else {
			Err(self.invalid_type(&visitor))
		}
	}

	/**
	An instant or a duration for `soml::Instant` or `soml::Duration`, which reads only from that type, never from a string.
	*/
	fn visit_token<V: Visitor<'de>>(self, name: &str, visitor: V) -> Result<V::Value, Error> {
		match (name, &self.node.kind) {
			(crate::instant::TOKEN, Kind::Instant(value)) => {
				visitor.visit_string(value.to_string())
			}
			(crate::duration::TOKEN, Kind::Duration(value)) => {
				visitor.visit_string(value.to_string())
			}
			(crate::instant::TOKEN, _) => Err(self.invalid_type(&"an instant")),
			_ => Err(self.invalid_type(&"a duration")),
		}
	}

	/**
	A SOML duration for a `std::time::Duration`, which serde reads as a struct with `secs` and `nanos`.
	*/
	fn visit_std_duration<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		let Kind::Duration(duration) = self.node.kind else {
			return self.visit_if(|kind| matches!(kind, Kind::Object(_)), visitor);
		};

		let Ok(nanoseconds) = u64::try_from(duration.nanoseconds()) else {
			return Err(Error::data(format!(
				"The duration {duration} is negative, which a std::time::Duration cannot be"
			)));
		};

		visitor.visit_map(MapDeserializer::new(
			[
				("secs", nanoseconds / 1_000_000_000),
				("nanos", nanoseconds % 1_000_000_000),
			]
			.into_iter(),
		))
	}

	fn visit_enum<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		let source = self.source;

		match self.node.kind {
			Kind::String(Cow::Borrowed(variant)) => {
				visitor.visit_enum(BorrowedStrDeserializer::new(variant))
			}
			Kind::String(Cow::Owned(variant)) => {
				visitor.visit_enum(StringDeserializer::new(variant))
			}
			Kind::Object(object) if object.members.len() == 1 => {
				let (key, member) = object
					.members
					.into_iter()
					.next()
					.expect("the object has one member");

				visitor.visit_enum(VariantDeserializer {
					key,
					member,
					source,
				})
			}
			_ => Err(self
				.invalid_type(&"an enum variant, which is a string, or an object with one member")),
		}
	}
}

/**
A bool or an integer type. `visit_any` gives an instant or a duration as its text, for types that read text, so these name the instant or the duration in their type error instead.
*/
macro_rules! deserialize_scalar {
	($($method:ident)*) => {
		$(
			fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
				self.located(|this| this.visit_if(|kind| !matches!(kind, Kind::Instant(_) | Kind::Duration(_)), visitor))
			}
		)*
	};
}

impl<'de> Deserializer<'de> for NodeDeserializer<'de> {
	type Error = Error;

	fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		self.located(|this| this.visit_any(visitor))
	}

	fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		self.located(|this| this.visit_float(visitor, true))
	}

	fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		self.located(|this| this.visit_float(visitor, false))
	}

	fn deserialize_char<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		self.deserialize_str(visitor)
	}

	fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		self.located(|this| {
			this.visit_if(
				|kind| matches!(kind, Kind::String(_) | Kind::Instant(_) | Kind::Duration(_)),
				visitor,
			)
		})
	}

	fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		self.deserialize_str(visitor)
	}

	fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		self.located(|this| {
			this.visit_if(
				|kind| matches!(kind, Kind::String(_) | Kind::Array(_)),
				visitor,
			)
		})
	}

	fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		self.deserialize_bytes(visitor)
	}

	fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		if matches!(self.node.kind, Kind::Null) {
			return visitor.visit_none();
		}

		visitor.visit_some(self)
	}

	fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		self.located(|this| match this.node.kind {
			Kind::Null => visitor.visit_unit(),
			_ => Err(this.invalid_type(&visitor)),
		})
	}

	fn deserialize_unit_struct<V: Visitor<'de>>(
		self,
		_name: &'static str,
		visitor: V,
	) -> Result<V::Value, Error> {
		self.deserialize_unit(visitor)
	}

	fn deserialize_newtype_struct<V: Visitor<'de>>(
		self,
		name: &'static str,
		visitor: V,
	) -> Result<V::Value, Error> {
		match name {
			crate::value::TOKEN => self.located(|this| this.visit_value(visitor)),
			crate::spanned::TOKEN => visitor.visit_map(SpannedDeserializer {
				span: self.node.offset.get().map(|start| (start, self.node.end)),
				value: Some(self),
				step: 0,
			}),
			crate::instant::TOKEN | crate::duration::TOKEN => {
				self.located(|this| this.visit_token(name, visitor))
			}
			_ => visitor.visit_newtype_struct(self),
		}
	}

	fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		self.located(|this| this.visit_if(|kind| matches!(kind, Kind::Array(_)), visitor))
	}

	fn deserialize_tuple<V: Visitor<'de>>(
		self,
		_length: usize,
		visitor: V,
	) -> Result<V::Value, Error> {
		self.deserialize_seq(visitor)
	}

	fn deserialize_tuple_struct<V: Visitor<'de>>(
		self,
		_name: &'static str,
		_length: usize,
		visitor: V,
	) -> Result<V::Value, Error> {
		self.deserialize_seq(visitor)
	}

	fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		self.located(|this| this.visit_if(|kind| matches!(kind, Kind::Object(_)), visitor))
	}

	fn deserialize_struct<V: Visitor<'de>>(
		self,
		name: &'static str,
		fields: &'static [&'static str],
		visitor: V,
	) -> Result<V::Value, Error> {
		// The shape serde gives `std::time::Duration`, in any field order, as the writer recognizes it.
		if name == "Duration"
			&& fields.len() == 2
			&& fields.contains(&"secs")
			&& fields.contains(&"nanos")
		{
			return self.located(|this| this.visit_std_duration(visitor));
		}

		self.deserialize_map(visitor)
	}

	fn deserialize_enum<V: Visitor<'de>>(
		self,
		_name: &'static str,
		_variants: &'static [&'static str],
		visitor: V,
	) -> Result<V::Value, Error> {
		self.located(|this| this.visit_enum(visitor))
	}

	fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		self.deserialize_str(visitor)
	}

	fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		visitor.visit_unit()
	}

	deserialize_scalar! {
		deserialize_bool deserialize_i8 deserialize_i16 deserialize_i32 deserialize_i64 deserialize_i128
		deserialize_u8 deserialize_u16 deserialize_u32 deserialize_u64 deserialize_u128
	}
}

struct SeqDeserializer<'de> {
	items: std::vec::IntoIter<Node<'de>>,
	source: Option<&'de str>,
}

impl<'de> SeqAccess<'de> for SeqDeserializer<'de> {
	type Error = Error;

	fn next_element_seed<T: DeserializeSeed<'de>>(
		&mut self,
		seed: T,
	) -> Result<Option<T::Value>, Error> {
		let Some(node) = self.items.next() else {
			return Ok(None);
		};

		// An error that a type makes after reading, such as a check in `#[serde(try_from)]`, is about this item.
		let offset = node.offset;

		seed.deserialize(NodeDeserializer {
			node,
			source: self.source,
		})
		.map(Some)
		.map_err(|error| error.or_at(self.source, offset.get()))
	}

	fn size_hint(&self) -> Option<usize> {
		Some(self.items.len())
	}
}

struct ObjectDeserializer<'de> {
	members: std::vec::IntoIter<(Cow<'de, str>, Member<'de>)>,
	value: Option<Node<'de>>,
	source: Option<&'de str>,
}

impl<'de> ObjectDeserializer<'de> {
	fn new(object: NodeObject<'de>, source: Option<&'de str>) -> Self {
		Self {
			members: object.members.into_iter(),
			value: None,
			source,
		}
	}
}

impl<'de> MapAccess<'de> for ObjectDeserializer<'de> {
	type Error = Error;

	fn next_key_seed<K: DeserializeSeed<'de>>(
		&mut self,
		seed: K,
	) -> Result<Option<K::Value>, Error> {
		let Some((key, member)) = self.members.next() else {
			return Ok(None);
		};

		self.value = Some(member.value);

		seed.deserialize(KeyDeserializer { key })
			.map(Some)
			.map_err(|error| error.or_at(self.source, member.key_offset.get()))
	}

	fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
		let node = self
			.value
			.take()
			.expect("serde calls next_key_seed before next_value_seed");

		// An error that a type makes after reading, such as a check in `#[serde(try_from)]`, is about this value.
		let offset = node.offset;

		seed.deserialize(NodeDeserializer {
			node,
			source: self.source,
		})
		.map_err(|error| error.or_at(self.source, offset.get()))
	}

	fn size_hint(&self) -> Option<usize> {
		Some(self.members.len())
	}
}

/**
A map key, which is always a string. It reads into an integer type only from canonical decimal text, such as `404` or `-1`, and into a bool from `true` or `false`.
*/
struct KeyDeserializer<'de> {
	key: Cow<'de, str>,
}

impl KeyDeserializer<'_> {
	/**
	Reads the key as an integer of the smallest serde type that holds it.
	*/
	fn visit_integer<'de, V: Visitor<'de>>(&self, visitor: V) -> Result<V::Value, Error> {
		let key = self.key.as_ref();
		let digits = key.strip_prefix('-').unwrap_or(key);
		let is_canonical = !digits.is_empty()
			&& digits.bytes().all(|byte| byte.is_ascii_digit())
			&& (digits == "0" || !digits.starts_with('0'))
			&& key != "-0";

		if is_canonical {
			if let Ok(value) = key.parse() {
				return visitor.visit_i64(value);
			}

			if let Ok(value) = key.parse() {
				return visitor.visit_u64(value);
			}

			if let Ok(value) = key.parse() {
				return visitor.visit_i128(value);
			}

			if let Ok(value) = key.parse() {
				return visitor.visit_u128(value);
			}
		}

		Err(Error::data(format!(
			"Expected the key “{}” to be an integer in decimal, like 404",
			crate::abbreviate(key, 40)
		)))
	}
}

macro_rules! deserialize_integer_key {
	($($method:ident)*) => {
		$(
			fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
				self.visit_integer(visitor)
			}
		)*
	};
}

impl<'de> Deserializer<'de> for KeyDeserializer<'de> {
	type Error = Error;

	fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		match self.key {
			Cow::Borrowed(key) => visitor.visit_borrowed_str(key),
			Cow::Owned(key) => visitor.visit_string(key),
		}
	}

	deserialize_integer_key! {
		deserialize_i8 deserialize_i16 deserialize_i32 deserialize_i64 deserialize_i128
		deserialize_u8 deserialize_u16 deserialize_u32 deserialize_u64 deserialize_u128
	}

	fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		match self.key.as_ref() {
			"true" => visitor.visit_bool(true),
			"false" => visitor.visit_bool(false),
			key => Err(Error::data(format!(
				"Expected the key “{}” to be true or false",
				crate::abbreviate(key, 40)
			))),
		}
	}

	fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		visitor.visit_some(self)
	}

	fn deserialize_newtype_struct<V: Visitor<'de>>(
		self,
		name: &'static str,
		visitor: V,
	) -> Result<V::Value, Error> {
		// A key is a string, and these read only from their own types.
		match name {
			crate::instant::TOKEN => Err(Error::data(
				"A key is a string, so it cannot be read as an instant",
			)),
			crate::duration::TOKEN => Err(Error::data(
				"A key is a string, so it cannot be read as a duration",
			)),
			_ => visitor.visit_newtype_struct(self),
		}
	}

	fn deserialize_enum<V: Visitor<'de>>(
		self,
		_name: &'static str,
		_variants: &'static [&'static str],
		visitor: V,
	) -> Result<V::Value, Error> {
		match self.key {
			Cow::Borrowed(key) => visitor.visit_enum(BorrowedStrDeserializer::new(key)),
			Cow::Owned(key) => visitor.visit_enum(StringDeserializer::new(key)),
		}
	}

	forward_to_deserialize_any! {
		f32 f64 char str string bytes byte_buf unit unit_struct seq tuple tuple_struct map struct identifier ignored_any
	}
}

/**
An enum variant written as an object with one member, `{variant: value}`.
*/
struct VariantDeserializer<'de> {
	key: Cow<'de, str>,
	member: Member<'de>,
	source: Option<&'de str>,
}

impl<'de> EnumAccess<'de> for VariantDeserializer<'de> {
	type Error = Error;
	type Variant = NodeDeserializer<'de>;

	fn variant_seed<V: DeserializeSeed<'de>>(
		self,
		seed: V,
	) -> Result<(V::Value, Self::Variant), Error> {
		let variant = seed
			.deserialize(KeyDeserializer { key: self.key })
			.map_err(|error| error.or_at(self.source, self.member.key_offset.get()))?;

		Ok((
			variant,
			NodeDeserializer {
				node: self.member.value,
				source: self.source,
			},
		))
	}
}

impl<'de> VariantAccess<'de> for NodeDeserializer<'de> {
	type Error = Error;

	fn unit_variant(self) -> Result<(), Error> {
		self.located(|this| match this.node.kind {
			Kind::Null => Ok(()),
			_ => Err(this.invalid_type(&"null for a unit variant")),
		})
	}

	fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Error> {
		self.located(|this| seed.deserialize(this))
	}

	fn tuple_variant<V: Visitor<'de>>(self, _length: usize, visitor: V) -> Result<V::Value, Error> {
		self.deserialize_seq(visitor)
	}

	fn struct_variant<V: Visitor<'de>>(
		self,
		_fields: &'static [&'static str],
		visitor: V,
	) -> Result<V::Value, Error> {
		self.deserialize_map(visitor)
	}
}

/**
An instant or a duration for `Value`'s visitor, as a variant named by its private token, which no other format produces.
*/
struct TokenEnum {
	name: &'static str,
	text: String,
}

impl<'de> EnumAccess<'de> for TokenEnum {
	type Error = Error;
	type Variant = Self;

	fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Self), Error> {
		let name: BorrowedStrDeserializer<'_, Error> = BorrowedStrDeserializer::new(self.name);
		Ok((seed.deserialize(name)?, self))
	}
}

impl<'de> VariantAccess<'de> for TokenEnum {
	type Error = Error;

	fn unit_variant(self) -> Result<(), Error> {
		Err(de::Error::custom(
			"expected the text of an instant or a duration",
		))
	}

	fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Error> {
		seed.deserialize(self.text.into_deserializer())
	}

	fn tuple_variant<V: Visitor<'de>>(
		self,
		_length: usize,
		_visitor: V,
	) -> Result<V::Value, Error> {
		Err(de::Error::custom(
			"expected the text of an instant or a duration",
		))
	}

	fn struct_variant<V: Visitor<'de>>(
		self,
		_fields: &'static [&'static str],
		_visitor: V,
	) -> Result<V::Value, Error> {
		Err(de::Error::custom(
			"expected the text of an instant or a duration",
		))
	}
}

impl Node<'_> {
	/**
	Converts a parsed tree into a `Value`, directly, without going through serde.
	*/
	pub(crate) fn into_value(self) -> Value {
		match self.kind {
			Kind::Null => Value::Null,
			Kind::Bool(value) => Value::Bool(value),
			Kind::Int(value) => Value::Int(value),
			Kind::Float(value) => Value::Float(value),
			Kind::String(value) => Value::String(value.into_owned()),
			Kind::Instant(value) => Value::Instant(value),
			Kind::Duration(value) => Value::Duration(value),
			Kind::Array(items) => Value::Array(items.into_iter().map(Node::into_value).collect()),
			Kind::Object(object) => Value::Object(
				object
					.members
					.into_iter()
					.map(|(key, member)| (key.into_owned(), member.value.into_value()))
					.collect(),
			),
		}
	}
}

impl From<Value> for Node<'static> {
	fn from(value: Value) -> Self {
		let kind = match value {
			Value::Null => Kind::Null,
			Value::Bool(value) => Kind::Bool(value),
			Value::Int(value) => Kind::Int(value),
			Value::Float(value) => Kind::Float(value),
			Value::String(value) => Kind::String(Cow::Owned(value)),
			Value::Instant(value) => Kind::Instant(value),
			Value::Duration(value) => Kind::Duration(value),
			Value::Array(items) => Kind::Array(items.into_iter().map(Self::from).collect()),
			Value::Object(object) => Kind::Object(NodeObject::new(
				object
					.into_iter()
					.map(|(key, value)| {
						(
							Cow::Owned(key),
							Member {
								key_offset: Offset::NONE,
								value: Self::from(value),
							},
						)
					})
					.collect(),
			)),
		};

		kind.into()
	}
}

/**
A value with its byte range for `Spanned`, as a map of the start, the end, and the value. The start and the end are left out for a value that did not come from a document. The value is read with the same deserializer, so it keeps its SOML type, such as an instant.
*/
struct SpannedDeserializer<'de> {
	span: Option<(usize, usize)>,
	value: Option<NodeDeserializer<'de>>,
	/**
	How many keys have been given.
	*/
	step: usize,
}

impl<'de> MapAccess<'de> for SpannedDeserializer<'de> {
	type Error = Error;

	fn next_key_seed<K: DeserializeSeed<'de>>(
		&mut self,
		seed: K,
	) -> Result<Option<K::Value>, Error> {
		// Without a span, only the value is given.
		if self.span.is_none() && self.step == 0 {
			self.step = 2;
		}

		let key = match self.step {
			0 => crate::spanned::START,
			1 => crate::spanned::END,
			2 => crate::spanned::VALUE,
			_ => return Ok(None),
		};

		self.step += 1;
		seed.deserialize(BorrowedStrDeserializer::new(key))
			.map(Some)
	}

	fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
		let (start, end) = self.span.unwrap_or_default();

		match self.step {
			1 => seed.deserialize((start as u64).into_deserializer()),
			2 => seed.deserialize((end as u64).into_deserializer()),
			_ => seed.deserialize(
				self.value
					.take()
					.expect("serde calls next_key_seed before next_value_seed"),
			),
		}
	}
}
