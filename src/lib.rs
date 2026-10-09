/*!
SOML, a config format for humans: a strict reader, serde support, canonical form, and a lossless syntax tree.

SOML keeps JSON's tree of objects and arrays and its strictness, and adds comments, bare keys, literal strings, block strings, instants, durations, and line breaks as separators, with optional commas. A value's type comes from its syntax, never from its content. See the [specification](https://github.com/soml-lang/soml), v0.1.

```rust
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Config {
	name: String,
	replicas: u32,
	timeout: std::time::Duration,
	deployed_at: soml::Instant,
	labels: Vec<String>,
}

let config: Config = soml::from_str("
## The edge service.
name: 'api-gateway'
replicas: 3
timeout: 1m30s
deployed-at: 2026-09-19T14:00:00Z
labels: ['prod', 'eu-west']
")?;

assert_eq!(config.replicas, 3);
assert_eq!(config.timeout.as_secs(), 90);
# Ok::<(), soml::Error>(())
```

# Reading

[`from_str`] and [`from_slice`] read a document into any type that implements `Deserialize`, and [`Value`] holds a document of unknown shape. Every rule in the spec is checked: duplicate keys, the int64 range, instant and duration ranges, raw control characters, and more. An error has a line and a column, also when the document is valid but does not fit the type:

```rust
#[derive(serde::Deserialize, Debug)]
struct Server {
	port: u16,
}

let error = soml::from_str::<Server>("port: 70000").unwrap_err();
assert_eq!(error.to_string(), "invalid value: integer `70000`, expected u16 at line 1, column 7");
```

# Writing

[`to_string`] writes a document for people to read: one member or item per line, tab indentation, and no commas, because a line break separates members and items. Members keep the order the value gives them, so a struct is written in the order of its fields.

[`to_string_canonical`] writes canonical form, the one text the spec defines for a value. It is the same, except that members are sorted by key, so the same value always gives the same bytes. Use it to hash, sign, or compare documents.

```rust
#[derive(serde::Serialize)]
struct Server {
	port: u16,
	hosts: Vec<&'static str>,
}

let server = Server { port: 8080, hosts: vec!["a", "b"] };
assert_eq!(soml::to_string(&server)?, "port: 8080\nhosts: [\n\t'a'\n\t'b'\n]\n");
assert_eq!(soml::to_string_canonical(&server)?, "hosts: [\n\t'a'\n\t'b'\n]\nport: 8080\n");
# Ok::<(), soml::Error>(())
```

A map is written in the order it iterates in. A `HashMap` iterates in a different order on each run, so use [`to_string_canonical`], a `BTreeMap`, or an `IndexMap` when the order must not change.

# Types

| Rust | SOML |
|---|---|
| `bool` | bool |
| `i8` to `i128`, `u8` to `u128` | int, which is 64-bit, so a larger value cannot be written |
| `f32`, `f64` | float. An int reads into a float only when it converts exactly, and a float never reads into an int. |
| `String`, `&str`, `char` | string |
| `Option<T>` | `null` or the value |
| `()`, a unit struct | `null` |
| `Vec<T>`, a tuple | array |
| a struct, `HashMap`, `BTreeMap` | object. A map key can also be a char, an integer, a bool, or a unit enum variant. |
| an enum | a string for a unit variant, and a one-member object for the others |
| [`Instant`] | instant |
| [`Duration`], `std::time::Duration` | duration. A negative duration does not fit a `std::time::Duration`. serde gives a `std::time::Duration` no type of its own, so it is recognized by its shape: a struct named `Duration` with the fields `secs` and `nanos`. A struct of your own with that shape is written as a duration too, and it reads back. |
| [`Value`] | any value |

Fields of jiff, chrono, and humantime types, such as `jiff::Timestamp`, `jiff::SignedDuration`, and `chrono::DateTime<Utc>`, read instants and durations too, because they ask for text, and an instant or a duration gives its canonical text. humantime reads only instants from 1970 on. The cost of this is that a `String` field also accepts an instant or a duration. `chrono::TimeDelta` does not ask for text, so it needs `soml::chrono::time_delta` to read a duration too. Those types write themselves as strings, so use [`Instant`] and [`Duration`] to write native instants and durations.

`#[serde(flatten)]`, untagged enums, and internally tagged enums read values through serde's buffer, which has no instant or duration type and no positions. There, an instant or a duration is its text: a [`Value`] holds it as a string, [`Instant`] and [`Duration`] still read it, and a `std::time::Duration` cannot be read. An int reads into a float even when the float does not hold it exactly. An `f32` is read through an `f64`, as in `serde_json`, so 2 of the 4.3 billion `f32` values read back one step away. A [`Spanned`] value has no span.

# Syntax tree

[`Document`] is a lossless syntax tree, for tools that read and change documents. An unchanged document prints the exact text it was parsed from, and a changed one keeps the comments and the author's spelling outside the change. Every parsed node has its position in the text.

```rust
let mut document: soml::Document = "port: 0x1F90 # The default.\n".parse()?;
document.set(["port"], 8080)?;
assert_eq!(document.to_string(), "port: 8080 # The default.\n");
# Ok::<(), soml::Error>(())
```

See [`tree`] for the nodes, and for how whitespace and comments are kept.

[`format()`] and [`Document::format`] are the spec's formatter: they change layout and nothing else.

# Positions and errors

[`Spanned`] reads any value with the byte range it came from, so a check your app makes after reading can point at the right place, with [`Error::with_position`]. Every [`Error`] has an [`ErrorKind`], and [`Error::code_frame`] shows where it is, for a terminal.

# More

- [`soml!`] builds a [`Value`] with JSON-like syntax.
- [`from_reader`], [`to_writer`], and [`to_writer_canonical`] read from an `io::Read` and write to an `io::Write`.
- The `jiff` and `chrono` features add conversions, and modules for `#[serde(with)]` that write their types as native instants and durations: `soml::jiff` and `soml::chrono`.

# Format

- File extension: `.soml`
- Encoding: UTF-8 without a byte order mark, with LF line endings
- Media type: `application/soml`
- Uniform type identifier: `com.sindresorhus.soml`

# Limits

- Nesting is limited to 100 levels, as the spec requires. Every object and array counts, including the document's own.
- Keys are compared byte for byte, so two keys that differ only in Unicode normalization are different keys, as the spec requires.
*/

#![forbid(unsafe_code)]
#![allow(clippy::tabs_in_doc_comments)]
#![warn(missing_docs)]
#![warn(clippy::missing_errors_doc)]
#![warn(clippy::missing_panics_doc)]
#![warn(clippy::doc_markdown)]

/**
A module for `#[serde(with = "…")]` that writes a type of another crate as a native SOML instant or duration, and reads it only from one, through a conversion to this crate's type. It has an `option` module inside for an `Option` of the type.
*/
#[cfg(any(feature = "jiff", feature = "chrono"))]
macro_rules! serde_with_module {
	($(#[$attribute:meta])* $name:ident, $type:ty, $soml:ty) => {
		$(#[$attribute])*
		pub mod $name {
			use serde_core::de::{Deserialize, Deserializer, Error as _};
			use serde_core::ser::{Error as _, Serialize, Serializer};

			/**
			Writes the value as a native SOML value.

			# Errors

			Returns an error when the value is outside the range of a SOML value.
			*/
			pub fn serialize<S: Serializer>(value: &$type, serializer: S) -> Result<S::Ok, S::Error> {
				<$soml>::try_from(*value).map_err(S::Error::custom)?.serialize(serializer)
			}

			/**
			Reads the value from a native SOML value.

			# Errors

			Returns an error when the document has another type of value there, or a value outside the range of the type.
			*/
			pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<$type, D::Error> {
				<$type>::try_from(<$soml>::deserialize(deserializer)?).map_err(D::Error::custom)
			}

			/**
			The same, for an `Option`, which is `null` when it is `None`. With `#[serde(default)]`, a missing member reads as `None` too.
			*/
			pub mod option {
				use super::*;

				/**
				Writes the value as a native SOML value, or `null`.

				# Errors

				Returns an error when the value is outside the range of a SOML value.
				*/
				pub fn serialize<S: Serializer>(value: &Option<$type>, serializer: S) -> Result<S::Ok, S::Error> {
					value.map(<$soml>::try_from).transpose().map_err(S::Error::custom)?.serialize(serializer)
				}

				/**
				Reads the value from a native SOML value, or `null`.

				# Errors

				Returns an error when the document has another type of value there, or a value outside the range of the type.
				*/
				pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<$type>, D::Error> {
					Option::<$soml>::deserialize(deserializer)?.map(<$type>::try_from).transpose().map_err(D::Error::custom)
				}
			}
		}
	};
}

#[cfg(feature = "chrono")]
pub mod chrono;
mod de;
mod document;
mod duration;
mod error;
mod instant;
#[cfg(feature = "jiff")]
pub mod jiff;
mod macros;
mod parse;
mod scalar;
mod ser;
mod spanned;
pub mod tree;
mod value;
mod write;

pub use document::{Document, PathSegment};
pub use duration::Duration;
pub use error::{Error, ErrorKind, LineColumn};
pub use instant::Instant;
pub use spanned::Spanned;
pub use value::{Index, Object, Value};

use serde_core::de::{Deserialize, DeserializeOwned};
use serde_core::ser::Serialize;

/**
Pulls the readme in as a doctest, so its examples are compiled and run by `cargo test`. Nothing is rendered; this only exists so the readme cannot drift from the API.
*/
// The readme shows the `jiff` feature, so its examples need it.
#[cfg(all(doctest, feature = "jiff"))]
#[doc = include_str!("../readme.md")]
#[doc(hidden)]
pub struct ReadmeDoctests;

/**
Reads a document into `T`.

A `'...'` or `"..."` string without escapes is borrowed from `text`, so `T` can hold `&str` fields. Other strings are copied, so use `String` or `Cow<str>` when a document may hold them.

# Errors

Returns an error when `text` is not a valid document, or when the document does not fit `T`. The error has the line and column of the problem. To find the error that comes first, such a document is read a second time, so `T`'s `Deserialize` impl runs twice for it.
*/
pub fn from_str<'de, T: Deserialize<'de>>(text: &'de str) -> Result<T, Error> {
	// Most documents are valid and fit their type, so they are read in one pass, without a tree. After any error, the document is read again through the tree, which gives the error that a check of the whole document gives first.
	if let Some(value) = de::stream::read(text) {
		return Ok(value);
	}

	// An error that a type makes after reading, such as a check in `#[serde(try_from)]`, is about the whole document.
	de::NodeDeserializer {
		node: parse::parse(text)?,
		source: Some(text),
	}
	.located(T::deserialize)
}

/**
Reads a document from UTF-8 bytes into `T`.

# Errors

Returns an error when `bytes` is not UTF-8, when it is not a valid document, or when the document does not fit `T`.
*/
pub fn from_slice<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, Error> {
	from_str(parse::decode(bytes)?)
}

/**
Reads a document from a reader into `T`.

It reads everything first, because a document is only valid as a whole: an error at its end, such as a duplicate key, makes all of it invalid. So this is a convenience for `from_slice` on the bytes, and when you have the text already, [`from_str`] can borrow strings from it.

```rust
let config: std::collections::BTreeMap<String, u16> = soml::from_reader("port: 8080".as_bytes())?;
assert_eq!(config["port"], 8080);
# Ok::<(), soml::Error>(())
```

# Errors

Returns an error of kind [`ErrorKind::Io`] when reading fails, and the errors of [`from_slice`] otherwise.
*/
pub fn from_reader<T: DeserializeOwned>(mut reader: impl std::io::Read) -> Result<T, Error> {
	let mut bytes = Vec::new();
	reader.read_to_end(&mut bytes).map_err(Error::io)?;
	from_slice(&bytes)
}

/**
Converts a [`Value`] into `T`.

```rust
let value: soml::Value = "port: 8080".parse()?;

#[derive(serde::Deserialize)]
struct Server {
	port: u16,
}

let server: Server = soml::from_value(value)?;
assert_eq!(server.port, 8080);
# Ok::<(), soml::Error>(())
```

# Errors

Returns an error when the value does not fit `T`. The error has no position, because a `Value` does not remember where it came from.

A `Value` holds a float as an `f64`, so an `f32` is read through the `f64`'s shortest digits. For a document's text with more than 15 significant digits, that can be the `f32` next to the one [`from_str`] gives, which rounds the text itself.

A `Value` from a document is never nested more than 100 levels deep. One built by hand that is nested thousands of levels deep can overflow the stack here.
*/
pub fn from_value<T: DeserializeOwned>(value: Value) -> Result<T, Error> {
	T::deserialize(de::NodeDeserializer {
		node: value.into(),
		source: None,
	})
}

/**
Writes `value` as a document, for people to read. It follows every rule of canonical form except the order of members, which stay in the order the value gives them: a struct's fields in the order they are declared, and a map's entries in the order it iterates in. So a `BTreeMap` and a [`Value`] are sorted by key, an `IndexMap` keeps its insertion order, and a `HashMap` has a different order on each run. Use [`to_string_canonical`] for the same bytes every time.

```rust
#[derive(serde::Serialize)]
struct Package {
	name: &'static str,
	description: &'static str,
}

let text = soml::to_string(&Package { name: "soml", description: "A config format" })?;
assert_eq!(text, "name: 'soml'\ndescription: 'A config format'\n");
# Ok::<(), soml::Error>(())
```

# Errors

Returns an error when the value is not an object or an array, because a document is always a collection, or when it holds something SOML cannot represent: NaN, an integer outside the 64-bit range, a carriage return in a string or a key, two members with the same key, a map key that is not a string, a char, a bool, an integer, or a unit enum variant, or nesting deeper than 100 levels. To give the same error as [`to_string_canonical`], such a value is serialized a second time, so its `Serialize` impl runs twice for it. That is also the case for a `Serialize` impl that catches an error and goes on.
*/
pub fn to_string<T: ?Sized + Serialize>(value: &T) -> Result<String, Error> {
	// Writing the text as the value is serialized is faster than building a tree first. When that fails, or a type caught an error, the tree serializer writes the value, because the stream finds errors in another order, so its error can be a different one, and it keeps the text that was written before an error.
	match ser::stream::write(value) {
		Some(text) => Ok(text),
		None => write::document(&value.serialize(ser::NodeSerializer { depth: 0 })?),
	}
}

/**
Writes `value` as a document in canonical form, the one text the spec defines for a value: the same as [`to_string`], with the members of every object sorted by key. Two equal values always give the same bytes, so use it to hash, sign, or compare documents.

```rust
#[derive(serde::Serialize)]
struct Package {
	name: &'static str,
	description: &'static str,
}

let text = soml::to_string_canonical(&Package { name: "soml", description: "A config format" })?;
assert_eq!(text, "description: 'A config format'\nname: 'soml'\n");
# Ok::<(), soml::Error>(())
```

# Errors

Returns the errors of [`to_string`].
*/
pub fn to_string_canonical<T: ?Sized + Serialize>(value: &T) -> Result<String, Error> {
	let mut node = value.serialize(ser::NodeSerializer { depth: 0 })?;
	node.sort_members();
	write::document(&node)
}

/**
Writes `value` as a document to a writer, as [`to_string`] does.

```rust
let mut output = Vec::new();
soml::to_writer(&mut output, &std::collections::BTreeMap::from([("port", 8080)]))?;
assert_eq!(output, b"port: 8080\n");
# Ok::<(), soml::Error>(())
```

# Errors

Returns the errors of [`to_string`], and an error of kind [`ErrorKind::Io`] when writing fails. Nothing is written when the value cannot be written.
*/
pub fn to_writer<T: ?Sized + Serialize>(
	mut writer: impl std::io::Write,
	value: &T,
) -> Result<(), Error> {
	writer
		.write_all(to_string(value)?.as_bytes())
		.map_err(Error::io)
}

/**
Writes `value` as a document in canonical form to a writer, as [`to_string_canonical`] does.

# Errors

Returns the errors of [`to_string`], and an error of kind [`ErrorKind::Io`] when writing fails. Nothing is written when the value cannot be written.
*/
pub fn to_writer_canonical<T: ?Sized + Serialize>(
	mut writer: impl std::io::Write,
	value: &T,
) -> Result<(), Error> {
	writer
		.write_all(to_string_canonical(value)?.as_bytes())
		.map_err(Error::io)
}

/**
Formats a document: it changes layout and nothing else, so comments, member order, and the spelling of every value stay. See [`Document::format`].

```rust
assert_eq!(soml::format("a: {b: 1, c: [2,3]}  # Note.")?, "a: {b: 1, c: [2, 3]} # Note.\n");
assert_eq!(soml::format("a: {\nb: 1, c: [2,3]}")?, "a: {\n\tb: 1\n\tc: [2, 3]\n}\n");
# Ok::<(), soml::Error>(())
```

# Errors

Returns an error when `text` is not a valid document.
*/
pub fn format(text: &str) -> Result<String, Error> {
	Ok(text.parse::<Document>()?.format())
}

/**
Converts `value` into a [`Value`].

```rust
let value = soml::to_value(&[1, 2, 3])?;
assert_eq!(value, soml::Value::Array(vec![1.into(), 2.into(), 3.into()]));
# Ok::<(), soml::Error>(())
```

# Errors

Returns an error when the value holds something SOML cannot represent, as for [`to_string`]. A top-level scalar is not an error here, because a `Value` does not have to be a document.
*/
pub fn to_value<T: ?Sized + Serialize>(value: &T) -> Result<Value, Error> {
	value
		.serialize(ser::NodeSerializer { depth: 0 })
		.map(parse::Node::into_value)
}

/**
Shortens text quoted in an error message, so a huge token does not make a huge message. The length counts UTF-16 code units, as the JS reference implementation does, so that the messages are the same. The cut never splits a character.
*/
pub(crate) fn abbreviate(text: &str, maximum_length: usize) -> std::borrow::Cow<'_, str> {
	let mut length = 0;

	for (index, character) in text.char_indices() {
		length += character.len_utf16();

		if length > maximum_length {
			return format!("{}…", &text[..index]).into();
		}
	}

	text.into()
}
