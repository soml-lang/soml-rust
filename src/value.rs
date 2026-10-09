use crate::{Duration, Error, Instant};
use serde_core::de::{
	self, Deserialize, Deserializer, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor,
};
use serde_core::ser::{Serialize, Serializer};
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fmt::{self, Display};
use std::str::FromStr;

/**
A SOML object: members by key.

The order of members is not part of a value in SOML, so an object is a `BTreeMap`. It iterates in canonical order, and two objects with the same members are equal. Keys are compared byte for byte, with no Unicode normalization, as the spec requires.
*/
pub type Object = BTreeMap<String, Value>;

/**
Any SOML value.

```rust
let value: soml::Value = "port: 8080\nhosts: ['a', 'b']".parse()?;

assert_eq!(value.get("port").and_then(soml::Value::as_i64), Some(8080));
assert_eq!(value.get("hosts").and_then(|hosts| hosts.get(1)).and_then(soml::Value::as_str), Some("b"));
# Ok::<(), soml::Error>(())
```

An int and a float are different types, as in the spec: `Value::Int(3)` and `Value::Float(3.0)` are not equal, and `as_f64()` does not convert an int.
*/
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Value {
	/**
	`null`.
	*/
	#[default]
	Null,
	/**
	`true` or `false`.
	*/
	Bool(bool),
	/**
	An int.
	*/
	Int(i64),
	/**
	A float. A float that came from a document is never NaN, and it is never negative zero.
	*/
	Float(f64),
	/**
	A string.
	*/
	String(String),
	/**
	An instant.
	*/
	Instant(Instant),
	/**
	A duration.
	*/
	Duration(Duration),
	/**
	An array.
	*/
	Array(Vec<Self>),
	/**
	An object.
	*/
	Object(Object),
}

/**
Something that can index into a `Value`: a key for an object, or a position for an array.
*/
pub trait Index: private::Sealed {
	#[doc(hidden)]
	fn index_into(self, value: &Value) -> Option<&Value>;

	#[doc(hidden)]
	fn index_into_mut(self, value: &mut Value) -> Option<&mut Value>;
}

impl Index for &str {
	fn index_into(self, value: &Value) -> Option<&Value> {
		value.as_object()?.get(self)
	}

	fn index_into_mut(self, value: &mut Value) -> Option<&mut Value> {
		value.as_object_mut()?.get_mut(self)
	}
}

impl Index for &String {
	fn index_into(self, value: &Value) -> Option<&Value> {
		self.as_str().index_into(value)
	}

	fn index_into_mut(self, value: &mut Value) -> Option<&mut Value> {
		self.as_str().index_into_mut(value)
	}
}

impl Index for usize {
	fn index_into(self, value: &Value) -> Option<&Value> {
		value.as_array()?.get(self)
	}

	fn index_into_mut(self, value: &mut Value) -> Option<&mut Value> {
		value.as_array_mut()?.get_mut(self)
	}
}

mod private {
	pub trait Sealed {}
	impl Sealed for &str {}
	impl Sealed for &String {}
	impl Sealed for usize {}
}

impl Value {
	/**
	The member of an object with this key, or the item of an array at this position. `None` for a missing member or item, and for a value of another type.
	*/
	#[must_use]
	pub fn get(&self, index: impl Index) -> Option<&Self> {
		index.index_into(self)
	}

	/**
	A mutable reference to the member of an object with this key, or the item of an array at this position.
	*/
	pub fn get_mut(&mut self, index: impl Index) -> Option<&mut Self> {
		index.index_into_mut(self)
	}

	/**
	Whether the value is `null`.
	*/
	#[must_use]
	pub const fn is_null(&self) -> bool {
		matches!(self, Self::Null)
	}

	/**
	The bool, if the value is one.
	*/
	#[must_use]
	pub const fn as_bool(&self) -> Option<bool> {
		match self {
			Self::Bool(value) => Some(*value),
			_ => None,
		}
	}

	/**
	The int, if the value is one.
	*/
	#[must_use]
	pub const fn as_i64(&self) -> Option<i64> {
		match self {
			Self::Int(value) => Some(*value),
			_ => None,
		}
	}

	/**
	The float, if the value is one. An int is not converted, because an int and a float are different types.
	*/
	#[must_use]
	pub const fn as_f64(&self) -> Option<f64> {
		match self {
			Self::Float(value) => Some(*value),
			_ => None,
		}
	}

	/**
	The string, if the value is one.
	*/
	#[must_use]
	pub fn as_str(&self) -> Option<&str> {
		match self {
			Self::String(value) => Some(value),
			_ => None,
		}
	}

	/**
	The instant, if the value is one.
	*/
	#[must_use]
	pub const fn as_instant(&self) -> Option<Instant> {
		match self {
			Self::Instant(value) => Some(*value),
			_ => None,
		}
	}

	/**
	The duration, if the value is one.
	*/
	#[must_use]
	pub const fn as_duration(&self) -> Option<Duration> {
		match self {
			Self::Duration(value) => Some(*value),
			_ => None,
		}
	}

	/**
	The items, if the value is an array.
	*/
	#[must_use]
	pub const fn as_array(&self) -> Option<&Vec<Self>> {
		match self {
			Self::Array(value) => Some(value),
			_ => None,
		}
	}

	/**
	The items, if the value is an array, to change.
	*/
	pub const fn as_array_mut(&mut self) -> Option<&mut Vec<Self>> {
		match self {
			Self::Array(value) => Some(value),
			_ => None,
		}
	}

	/**
	The members, if the value is an object.
	*/
	#[must_use]
	pub const fn as_object(&self) -> Option<&Object> {
		match self {
			Self::Object(value) => Some(value),
			_ => None,
		}
	}

	/**
	The members, if the value is an object, to change.
	*/
	pub const fn as_object_mut(&mut self) -> Option<&mut Object> {
		match self {
			Self::Object(value) => Some(value),
			_ => None,
		}
	}
}

/**
Reads a document, which is always an object or an array.
*/
impl FromStr for Value {
	type Err = Error;

	fn from_str(text: &str) -> Result<Self, Error> {
		crate::parse::parse(text).map(crate::parse::Node::into_value)
	}
}

macro_rules! from_integer {
	($($type:ty),*) => {
		$(
			impl From<$type> for Value {
				fn from(value: $type) -> Self {
					Self::Int(value.into())
				}
			}
		)*
	};
}

from_integer!(i8, i16, i32, i64, u8, u16, u32);

impl From<f64> for Value {
	fn from(value: f64) -> Self {
		Self::Float(value)
	}
}

impl From<f32> for Value {
	/**
	Uses the shortest decimal that reads back as the same `f32`, so `0.1f32` becomes `0.1`, not `0.10000000149011612`.
	*/
	fn from(value: f32) -> Self {
		Self::Float(crate::ser::widen(value))
	}
}

impl From<bool> for Value {
	fn from(value: bool) -> Self {
		Self::Bool(value)
	}
}

impl From<String> for Value {
	fn from(value: String) -> Self {
		Self::String(value)
	}
}

impl From<&str> for Value {
	fn from(value: &str) -> Self {
		Self::String(value.to_owned())
	}
}

impl From<Instant> for Value {
	fn from(value: Instant) -> Self {
		Self::Instant(value)
	}
}

impl From<Duration> for Value {
	fn from(value: Duration) -> Self {
		Self::Duration(value)
	}
}

impl<T: Into<Self>> From<Vec<T>> for Value {
	fn from(value: Vec<T>) -> Self {
		Self::Array(value.into_iter().map(Into::into).collect())
	}
}

impl<T: Into<Self>, const N: usize> From<[T; N]> for Value {
	fn from(value: [T; N]) -> Self {
		Self::Array(value.into_iter().map(Into::into).collect())
	}
}

impl From<Object> for Value {
	fn from(value: Object) -> Self {
		Self::Object(value)
	}
}

impl<T: Into<Self>> From<Option<T>> for Value {
	fn from(value: Option<T>) -> Self {
		value.map_or(Self::Null, Into::into)
	}
}

impl<T: Into<Self>> FromIterator<T> for Value {
	fn from_iter<I: IntoIterator<Item = T>>(iterator: I) -> Self {
		Self::Array(iterator.into_iter().map(Into::into).collect())
	}
}

impl<K: Into<String>, V: Into<Self>> FromIterator<(K, V)> for Value {
	fn from_iter<I: IntoIterator<Item = (K, V)>>(iterator: I) -> Self {
		Self::Object(
			iterator
				.into_iter()
				.map(|(key, value)| (key.into(), value.into()))
				.collect(),
		)
	}
}

/**
Serializes a value's canonical text, with `collect_str`, so that it allocates nothing for a serializer that writes text directly.
*/
pub(crate) struct CanonicalText<'a, T: Display>(pub &'a T);

impl<T: Display> Serialize for CanonicalText<'_, T> {
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		serializer.collect_str(self.0)
	}
}

impl Serialize for Value {
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		match self {
			Self::Null => serializer.serialize_unit(),
			Self::Bool(value) => serializer.serialize_bool(*value),
			Self::Int(value) => serializer.serialize_i64(*value),
			Self::Float(value) => serializer.serialize_f64(*value),
			Self::String(value) => serializer.serialize_str(value),
			Self::Instant(value) => value.serialize(serializer),
			Self::Duration(value) => value.serialize(serializer),
			Self::Array(value) => serializer.collect_seq(value),
			Self::Object(value) => serializer.collect_map(value),
		}
	}
}

/**
The name that tells the SOML deserializer that a `Value` is being read, so that it gives an instant or a duration as itself, through `visit_enum`, rather than as its text.
*/
pub(crate) const TOKEN: &str = "$soml::Value";

impl<'de> Deserialize<'de> for Value {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		deserializer.deserialize_newtype_struct(TOKEN, ValueVisitor)
	}
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
	type Value = Value;

	fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("any SOML value")
	}

	fn visit_bool<E>(self, value: bool) -> Result<Value, E> {
		Ok(Value::Bool(value))
	}

	fn visit_i64<E>(self, value: i64) -> Result<Value, E> {
		Ok(Value::Int(value))
	}

	fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
		crate::ser::integer(value)
			.map(Value::Int)
			.map_err(E::custom)
	}

	fn visit_i128<E: de::Error>(self, value: i128) -> Result<Value, E> {
		crate::ser::integer(value)
			.map(Value::Int)
			.map_err(E::custom)
	}

	fn visit_u128<E: de::Error>(self, value: u128) -> Result<Value, E> {
		crate::ser::integer(value)
			.map(Value::Int)
			.map_err(E::custom)
	}

	// An `f32` from another format, with its own shortest digits, as `From<f32>` makes it.
	fn visit_f32<E>(self, value: f32) -> Result<Value, E> {
		Ok(Value::from(value))
	}

	fn visit_f64<E>(self, value: f64) -> Result<Value, E> {
		Ok(Value::Float(value))
	}

	fn visit_str<E>(self, value: &str) -> Result<Value, E> {
		Ok(Value::String(value.to_owned()))
	}

	fn visit_string<E>(self, value: String) -> Result<Value, E> {
		Ok(Value::String(value))
	}

	fn visit_unit<E>(self) -> Result<Value, E> {
		Ok(Value::Null)
	}

	fn visit_none<E>(self) -> Result<Value, E> {
		Ok(Value::Null)
	}

	fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
		Value::deserialize(deserializer)
	}

	fn visit_newtype_struct<D: Deserializer<'de>>(
		self,
		deserializer: D,
	) -> Result<Value, D::Error> {
		deserializer.deserialize_any(self)
	}

	fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
		let mut items = Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(4096));

		while let Some(item) = sequence.next_element()? {
			items.push(item);
		}

		Ok(Value::Array(items))
	}

	fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
		let mut object = Object::new();

		while let Some(key) = map.next_key::<String>()? {
			match object.entry(key) {
				Entry::Vacant(entry) => {
					entry.insert(map.next_value()?);
				}
				Entry::Occupied(entry) => {
					return Err(de::Error::custom(format!(
						"Duplicate key “{}”",
						entry.key()
					)));
				}
			}
		}

		Ok(Value::Object(object))
	}

	/**
	The SOML deserializer gives an instant or a duration as a variant named by its private token, which no other format produces.
	*/
	fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<Value, A::Error> {
		let (name, variant): (String, _) = data.variant()?;

		match name.as_str() {
			crate::instant::TOKEN => {
				let text: String = variant.newtype_variant()?;
				Instant::parse(&text)
					.map(Value::Instant)
					.map_err(de::Error::custom)
			}
			crate::duration::TOKEN => {
				let text: String = variant.newtype_variant()?;
				Duration::parse(&text)
					.map(Value::Duration)
					.map_err(de::Error::custom)
			}
			_ => Err(de::Error::custom("A SOML value cannot hold an enum")),
		}
	}
}
