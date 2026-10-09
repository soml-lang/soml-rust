/*!
A deserializer that reads a document in one pass, without building a tree, for the common case: a valid document that fits its type. It checks every rule with the same parser steps that build the tree, but it does not explain an error. After any error, `from_str` reads the document again through the tree, which gives the same error, with the same message and position, as a check of the whole document does.
*/

use super::{KeyDeserializer, NodeDeserializer};
use crate::Error;
use crate::parse::{Collection, ObjectKeys, Parser, check_characters};
use serde_core::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use std::borrow::Cow;

/**
Reads a document into `T`, or returns `None` when that fails in any way, so that the caller reads it again through the tree.
*/
pub(crate) fn read<'de, T: de::Deserialize<'de>>(source: &'de str) -> Option<T> {
	// The parser does not look for a byte order mark or a raw control character itself, so this check must come first, as in `parse::parse`.
	check_characters(source).ok()?;

	let mut stream = Stream {
		parser: Parser::new(source),
		source,
		keys: Vec::new(),
		depth: 0,
		failed: false,
	};

	stream.parser.start_document().ok()?;
	let value = T::deserialize(&mut stream).ok()?;
	stream.parser.end_document().ok()?;
	(!stream.failed).then_some(value)
}

/**
The error of every failed step. Nobody sees it, because the document is then read again through the tree.
*/
#[cold]
fn failure<T>(_: T) -> Error {
	de::Error::custom("the document is read again through the tree")
}

struct Stream<'de> {
	parser: Parser<'de>,
	source: &'de str,
	/**
	The keys of the objects that are being read, the innermost last, to find a duplicate.
	*/
	keys: Vec<Cow<'de, str>>,
	/**
	How many objects and arrays are open.
	*/
	depth: usize,
	/**
	Whether an error left any step. A type can swallow an error, as a `deserialize_with` that falls back to a default does, and that leaves the cursor inside a value.
	*/
	failed: bool,
}

/**
What starts at the cursor.
*/
enum Next {
	Scalar,
	Array,
	Object,
	/**
	The document's own object, which has no braces.
	*/
	BareObject,
}

impl<'de> Stream<'de> {
	#[inline]
	fn next(&self) -> Next {
		match self.parser.peek() {
			Some(b'[') => Next::Array,
			Some(b'{') => Next::Object,
			// The document is an object or an array, so it is an object without braces when it does not start with a bracket.
			_ if self.depth == 0 => Next::BareObject,
			_ => Next::Scalar,
		}
	}

	/**
	Runs a step, and remembers when it fails.
	*/
	fn watch<T>(&mut self, step: impl FnOnce(&mut Self) -> Result<T, Error>) -> Result<T, Error> {
		let result = step(self);
		self.failed |= result.is_err();
		result
	}

	/**
	Reads the value at the cursor into a tree, for the tree's deserializer. Every scalar is read this way, and so is a collection for a type that this deserializer does not read in one pass, such as `Spanned`, or that does not take a collection.
	*/
	#[inline]
	fn node(&mut self) -> Result<NodeDeserializer<'de>, Error> {
		let node = match self.next() {
			Next::BareObject => self.parser.parse_bare_object(),
			_ => self.parser.parse_value(self.depth + 1),
		}
		.map_err(failure)?;

		Ok(NodeDeserializer {
			node,
			source: Some(self.source),
		})
	}

	fn visit_array<V: Visitor<'de>>(&mut self, visitor: V) -> Result<V::Value, Error> {
		self.depth += 1;
		let start = self.parser.open_braced(self.depth).map_err(failure)?;

		let mut items = Items {
			stream: self,
			start,
			is_done: false,
		};

		// An error returns before `depth` is restored. That does no harm, because `watch` sets `failed`, and the document is then read again through the tree.
		let value = visitor.visit_seq(&mut items)?;

		// A visitor that stops early, such as a tuple's, leaves items, which is an error.
		if !items.is_done && items.has_item()? {
			return Err(failure(()));
		}

		self.depth -= 1;
		Ok(value)
	}

	fn visit_object<V: Visitor<'de>>(
		&mut self,
		visitor: V,
		is_bare: bool,
	) -> Result<V::Value, Error> {
		self.depth += 1;

		let start = if is_bare {
			None
		} else {
			Some(self.parser.open_braced(self.depth).map_err(failure)?)
		};

		let mut members = Members {
			keys: ObjectKeys::new(&self.keys),
			stream: self,
			start,
			is_first: is_bare,
			is_done: false,
			has_value: false,
		};

		// As in `visit_array`, an error returns before `keys` and `depth` are restored, which does no harm.
		let value = visitor.visit_map(&mut members)?;

		// A visitor that stops early leaves members, which is an error.
		if members.has_value || (!members.is_done && members.has_entry()?) {
			return Err(failure(()));
		}

		members.keys.end(&mut members.stream.keys);
		self.depth -= 1;
		Ok(value)
	}

	/**
	Reads the value at the cursor with `seed`. A type that returns without reading its value leaves the cursor where it was, and then whatever is there would go unchecked, so that is an error.
	*/
	fn read_value<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<T::Value, Error> {
		let start = self.parser.index;
		let value = seed.deserialize(&mut *self)?;

		if self.parser.index == start {
			return Err(failure(()));
		}

		Ok(value)
	}

	/**
	Whether the first key of the object at the cursor is `key`.
	*/
	fn first_key_is(&self, key: &str, is_bare: bool) -> bool {
		let mut parser = self.parser.clone();

		// This only looks ahead on a copy, so it skips the depth check. The read that follows checks the depth.
		if !is_bare && parser.open_braced(0).is_err() {
			return false;
		}

		parser
			.parse_entry_key()
			.is_ok_and(|(first, _)| first == key)
	}
}

/**
A type that takes no collection, so a collection is left to the tree's deserializer, which gives the type error. An ignored value is also read through the tree, so all of it is checked.
*/
macro_rules! deserialize_from_tree {
	($($method:ident)*) => {
		$(
			fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
				self.watch(|this| this.node()?.$method(visitor))
			}
		)*
	};
}

impl<'de> Deserializer<'de> for &mut Stream<'de> {
	type Error = Error;

	fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		self.watch(|this| match this.next() {
			Next::Array => this.visit_array(visitor),
			Next::Object => this.visit_object(visitor, false),
			Next::BareObject => this.visit_object(visitor, true),
			Next::Scalar => this.node()?.deserialize_any(visitor),
		})
	}

	fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		self.watch(|this| match this.next() {
			Next::Array => this.visit_array(visitor),
			_ => this.node()?.deserialize_bytes(visitor),
		})
	}

	fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		self.deserialize_bytes(visitor)
	}

	fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		self.watch(|this| match this.next() {
			Next::Scalar => this.node()?.deserialize_option(visitor),
			_ => visitor.visit_some(this),
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
		self.watch(|this| match (name, this.next()) {
			// A `Value` differs from any other type only in its scalars, an instant and a duration.
			(crate::value::TOKEN, Next::Array | Next::Object | Next::BareObject) => {
				this.deserialize_any(visitor)
			}
			(
				crate::value::TOKEN
				| crate::spanned::TOKEN
				| crate::instant::TOKEN
				| crate::duration::TOKEN,
				_,
			) => this.node()?.deserialize_newtype_struct(name, visitor),
			_ => visitor.visit_newtype_struct(this),
		})
	}

	fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
		self.watch(|this| match this.next() {
			Next::Array => this.visit_array(visitor),
			_ => this.node()?.deserialize_seq(visitor),
		})
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
		self.watch(|this| match this.next() {
			Next::Object => this.visit_object(visitor, false),
			Next::BareObject => this.visit_object(visitor, true),
			_ => this.node()?.deserialize_map(visitor),
		})
	}

	fn deserialize_struct<V: Visitor<'de>>(
		self,
		name: &'static str,
		fields: &'static [&'static str],
		visitor: V,
	) -> Result<V::Value, Error> {
		self.watch(|this| {
			let is_bare = match this.next() {
				Next::Object => false,
				Next::BareObject => true,
				_ => return this.node()?.deserialize_struct(name, fields, visitor),
			};

			// An adjacently tagged enum with its content first is reordered by the tree's deserializer.
			// A plain struct with two fields, whose second field comes first, also takes this path. That is slower, but it gives the same value.
			if let [_, content] = fields
				&& this.first_key_is(content, is_bare)
			{
				return this.node()?.deserialize_struct(name, fields, visitor);
			}

			this.visit_object(visitor, is_bare)
		})
	}

	fn deserialize_enum<V: Visitor<'de>>(
		self,
		name: &'static str,
		variants: &'static [&'static str],
		visitor: V,
	) -> Result<V::Value, Error> {
		self.watch(|this| this.node()?.deserialize_enum(name, variants, visitor))
	}

	deserialize_from_tree! {
		deserialize_bool deserialize_i8 deserialize_i16 deserialize_i32 deserialize_i64 deserialize_i128
		deserialize_u8 deserialize_u16 deserialize_u32 deserialize_u64 deserialize_u128
		deserialize_f32 deserialize_f64 deserialize_char deserialize_str deserialize_string
		deserialize_unit deserialize_identifier deserialize_ignored_any
	}
}

struct Items<'a, 'de> {
	stream: &'a mut Stream<'de>,
	/**
	The offset of the opening bracket.
	*/
	start: usize,
	/**
	Whether the closing bracket has been read.
	*/
	is_done: bool,
}

impl<'de> Items<'_, 'de> {
	#[inline]
	fn has_item(&mut self) -> Result<bool, Error> {
		self.stream
			.parser
			.next_braced_item(Collection::Array, self.start)
			.map_err(failure)
	}

	fn next_item<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>, Error> {
		// A visitor may ask again after the end, which must not read past the closing bracket.
		if self.is_done || !self.has_item()? {
			self.is_done = true;
			return Ok(None);
		}

		let value = self.stream.read_value(seed)?;

		self.stream
			.parser
			.after_braced_item(Collection::Array, self.start)
			.map_err(failure)?;

		Ok(Some(value))
	}
}

impl<'de> SeqAccess<'de> for Items<'_, 'de> {
	type Error = Error;

	fn next_element_seed<T: DeserializeSeed<'de>>(
		&mut self,
		seed: T,
	) -> Result<Option<T::Value>, Error> {
		// After an error that a type caught, the cursor stays where it failed, so a type that keeps asking would never get to the end. The document is read again through the tree anyway.
		if self.stream.failed {
			return Ok(None);
		}

		let result = self.next_item(seed);
		self.stream.failed |= result.is_err();
		result
	}
}

struct Members<'a, 'de> {
	stream: &'a mut Stream<'de>,
	/**
	The offset of the opening brace, or `None` for the document's own object, which has no braces.
	*/
	start: Option<usize>,
	keys: ObjectKeys<'de>,
	/**
	Whether the first entry of an object without braces is next, which has no separator before it.
	*/
	is_first: bool,
	/**
	Whether the end of the object has been read.
	*/
	is_done: bool,
	/**
	Whether a key has been read, and its value is next.
	*/
	has_value: bool,
}

impl<'de> Members<'_, 'de> {
	#[inline]
	fn has_entry(&mut self) -> Result<bool, Error> {
		let parser = &mut self.stream.parser;

		match self.start {
			Some(start) => parser.next_braced_item(Collection::Object, start),
			None if std::mem::take(&mut self.is_first) => Ok(true),
			None => parser.next_bare_entry(),
		}
		.map_err(failure)
	}

	/**
	Remembers a key of this object, and fails when it has the key already.
	*/
	#[inline]
	fn insert(&mut self, key: Cow<'de, str>) -> Result<(), Error> {
		// The stack of keys ends with the keys of this object, because an inner object removes its keys when it ends, and no key is read after a failure.
		if self.keys.insert(&mut self.stream.keys, key) {
			Ok(())
		} else {
			Err(failure(()))
		}
	}

	fn next_key<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, Error> {
		// The cursor is at the value of the key before, which must not be read as a key.
		if self.has_value {
			return Err(failure(()));
		}

		// A visitor may ask again after the end, which must not read past the closing brace.
		if self.is_done || !self.has_entry()? {
			self.is_done = true;
			return Ok(None);
		}

		let (key, _) = self.stream.parser.parse_entry_key().map_err(failure)?;
		self.insert(key.clone())?;
		self.has_value = true;
		seed.deserialize(KeyDeserializer { key }).map(Some)
	}

	fn next_value<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
		if !std::mem::take(&mut self.has_value) {
			return Err(failure(()));
		}

		let value = self.stream.read_value(seed)?;

		// In the document's own object, `has_entry` reads the separator before the next key instead.
		if let Some(start) = self.start {
			self.stream
				.parser
				.after_braced_item(Collection::Object, start)
				.map_err(failure)?;
		}

		Ok(value)
	}
}

impl<'de> MapAccess<'de> for Members<'_, 'de> {
	type Error = Error;

	fn next_key_seed<K: DeserializeSeed<'de>>(
		&mut self,
		seed: K,
	) -> Result<Option<K::Value>, Error> {
		// After an error that a type caught, the cursor stays where it failed, so a type that keeps asking would never get to the end. The document is read again through the tree anyway.
		if self.stream.failed {
			return Ok(None);
		}

		let result = self.next_key(seed);
		self.stream.failed |= result.is_err();
		result
	}

	fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
		let result = self.next_value(seed);
		self.stream.failed |= result.is_err();
		result
	}
}
