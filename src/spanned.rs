use serde_core::de::value::{
	BorrowedBytesDeserializer, BorrowedStrDeserializer, EnumAccessDeserializer,
	SeqAccessDeserializer,
};
use serde_core::de::{
	self, Deserialize, Deserializer, EnumAccess, IntoDeserializer, MapAccess, SeqAccess, Visitor,
};
use serde_core::ser::{Serialize, Serializer};
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut, Range};

/**
A value with the byte range it was read from, so an app can point at it when the value is valid SOML but not valid for the app.

```rust
#[derive(serde::Deserialize)]
struct Server {
	port: soml::Spanned<u16>,
}

let text = "port: 0";
let server: Server = soml::from_str(text)?;

assert_eq!(*server.port, 0);
assert_eq!(server.port.span(), Some(6..7));

let error = soml::Error::with_position("Port 0 is reserved", text, server.port.span().unwrap().start);
assert_eq!(error.code_frame(text).unwrap(), "> 1 | port: 0\n    |       ^");
# Ok::<(), soml::Error>(())
```

The span is `None` for a value that did not come from a SOML document: one read with [`from_value`](crate::from_value), or by another format, such as JSON, which gives the value alone. A format that gives a newtype struct's content instead, as serde's own deserializers do, can give any value but a map or a struct. It is also `None` for a map key, and inside `#[serde(flatten)]`, untagged enums, and internally tagged enums, because serde reads values there through a buffer that has no positions.

The span of a top-level object without braces runs from its first member to the end of its last value.

`Spanned` compares, orders, and hashes by the value alone, and it serializes as the value.
*/
#[derive(Clone, Default)]
pub struct Spanned<T> {
	value: T,
	span: Option<Range<usize>>,
}

/**
The name that tells the SOML deserializer to give a value with its byte range, as a map with the keys below.
*/
pub(crate) const TOKEN: &str = "$soml::Spanned";

/**
The keys of the map the SOML deserializer gives for a spanned value.
*/
pub(crate) const START: &str = "$soml::Spanned::start";
pub(crate) const END: &str = "$soml::Spanned::end";
pub(crate) const VALUE: &str = "$soml::Spanned::value";

impl<T> Spanned<T> {
	/**
	A value without a span, for a value that did not come from a document.
	*/
	#[must_use]
	pub const fn new(value: T) -> Self {
		Self { value, span: None }
	}

	/**
	The byte range of the value in the document it was read from, if it was read from a document.
	*/
	#[must_use]
	pub fn span(&self) -> Option<Range<usize>> {
		self.span.clone()
	}

	/**
	The value.
	*/
	#[must_use]
	pub const fn get_ref(&self) -> &T {
		&self.value
	}

	/**
	The value, to change.
	*/
	pub const fn get_mut(&mut self) -> &mut T {
		&mut self.value
	}

	/**
	The value, without its span.
	*/
	#[must_use]
	pub fn into_inner(self) -> T {
		self.value
	}
}

impl<T> Deref for Spanned<T> {
	type Target = T;

	fn deref(&self) -> &T {
		&self.value
	}
}

impl<T> DerefMut for Spanned<T> {
	fn deref_mut(&mut self) -> &mut T {
		&mut self.value
	}
}

impl<T> From<T> for Spanned<T> {
	fn from(value: T) -> Self {
		Self::new(value)
	}
}

impl<T: fmt::Debug> fmt::Debug for Spanned<T> {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter
			.debug_struct("Spanned")
			.field("value", &self.value)
			.field("span", &self.span)
			.finish()
	}
}

impl<T: fmt::Display> fmt::Display for Spanned<T> {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.value.fmt(formatter)
	}
}

impl<T: PartialEq> PartialEq for Spanned<T> {
	fn eq(&self, other: &Self) -> bool {
		self.value == other.value
	}
}

impl<T: Eq> Eq for Spanned<T> {}

impl<T: PartialOrd> PartialOrd for Spanned<T> {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		self.value.partial_cmp(&other.value)
	}
}

impl<T: Ord> Ord for Spanned<T> {
	fn cmp(&self, other: &Self) -> Ordering {
		self.value.cmp(&other.value)
	}
}

impl<T: Hash> Hash for Spanned<T> {
	fn hash<H: Hasher>(&self, state: &mut H) {
		self.value.hash(state);
	}
}

impl<T: Serialize> Serialize for Spanned<T> {
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		self.value.serialize(serializer)
	}
}

/**
Visits that read another format's value into a `Spanned`, for a deserializer that gives the content of a newtype struct, as serde's own deserializers do.
*/
macro_rules! visit_scalars {
	($($method:ident: $type:ty),*) => {
		$(
			fn $method<E: de::Error>(self, value: $type) -> Result<Spanned<T>, E> {
				T::deserialize(value.into_deserializer()).map(Spanned::new)
			}
		)*
	};
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Spanned<T> {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		struct SpannedVisitor<T>(PhantomData<T>);

		impl<'de, T: Deserialize<'de>> Visitor<'de> for SpannedVisitor<T> {
			type Value = Spanned<T>;

			fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
				formatter.write_str("a value")
			}

			/**
			Another format gives the value alone.
			*/
			fn visit_newtype_struct<D: Deserializer<'de>>(
				self,
				deserializer: D,
			) -> Result<Spanned<T>, D::Error> {
				T::deserialize(deserializer).map(Spanned::new)
			}

			/**
			The SOML deserializer gives the span, then the value, which is read with the same deserializer so it keeps its SOML type.
			*/
			fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Spanned<T>, A::Error> {
				let mut start = None;
				let mut end = None;

				while let Some(key) = map.next_key::<&str>()? {
					match key {
						START => start = Some(map.next_value()?),
						END => end = Some(map.next_value()?),
						VALUE => {
							let value = map.next_value()?;

							return Ok(Spanned {
								value,
								span: start.zip(end).map(|(start, end)| start..end),
							});
						}
						_ => return Err(de::Error::custom("expected a spanned value")),
					}
				}

				Err(de::Error::custom("expected a spanned value"))
			}

			fn visit_seq<A: SeqAccess<'de>>(self, sequence: A) -> Result<Spanned<T>, A::Error> {
				T::deserialize(SeqAccessDeserializer::new(sequence)).map(Spanned::new)
			}

			fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<Spanned<T>, A::Error> {
				T::deserialize(EnumAccessDeserializer::new(data)).map(Spanned::new)
			}

			// The borrowed visits keep the `'de` lifetime, so `T` can be a `&str` or a `&[u8]`, which `visit_str` and `visit_bytes` cannot give.
			fn visit_borrowed_str<E: de::Error>(self, value: &'de str) -> Result<Spanned<T>, E> {
				T::deserialize(BorrowedStrDeserializer::new(value)).map(Spanned::new)
			}

			fn visit_borrowed_bytes<E: de::Error>(self, value: &'de [u8]) -> Result<Spanned<T>, E> {
				T::deserialize(BorrowedBytesDeserializer::new(value)).map(Spanned::new)
			}

			fn visit_unit<E: de::Error>(self) -> Result<Spanned<T>, E> {
				T::deserialize(().into_deserializer()).map(Spanned::new)
			}

			fn visit_none<E: de::Error>(self) -> Result<Spanned<T>, E> {
				self.visit_unit()
			}

			fn visit_some<D: Deserializer<'de>>(
				self,
				deserializer: D,
			) -> Result<Spanned<T>, D::Error> {
				T::deserialize(deserializer).map(Spanned::new)
			}

			visit_scalars! {
				visit_bool: bool, visit_i8: i8, visit_i16: i16, visit_i32: i32, visit_i64: i64, visit_i128: i128,
				visit_u8: u8, visit_u16: u16, visit_u32: u32, visit_u64: u64, visit_u128: u128,
				visit_f32: f32, visit_f64: f64, visit_char: char, visit_str: &str, visit_string: String,
				visit_bytes: &[u8], visit_byte_buf: Vec<u8>
			}
		}

		deserializer.deserialize_newtype_struct(TOKEN, SpannedVisitor(PhantomData))
	}
}
