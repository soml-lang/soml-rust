/*!
The serializer for `to_string`, which writes the text as it goes, without the tree that `NodeSerializer` builds. It writes the same text as `write::document` writes for that tree, except for the text that a type gives under the private name of an instant or a duration (see `token`), and it fails on every value that the tree serializer rejects. It does not always give the same error, because it checks in another order, so `to_string` writes the value again through the tree when this fails, which gives the error. It also does that when a type caught an error and went on (see `Writer::failed`).
*/

use super::{KeySerializer, NodeSerializer, integer, widen};
use crate::parse::{MAX_FILTERED_KEYS, fingerprint};
use crate::scalar::is_bare_key;
use crate::write::{self, check_representable};
use crate::{Duration, Error, Instant};
use serde_core::ser::{self, Serialize};
use std::collections::HashSet;
use std::ops::Range;

/**
Writes `value` as a document, or returns `None` when the value cannot be written.
*/
pub(crate) fn write<T: ?Sized + Serialize>(value: &T) -> Option<String> {
	let mut writer = Writer {
		// Most documents are a few KB, and growing from nothing to that copies the text many times.
		output: String::with_capacity(1024),
		// Room for the keys of a few open objects, so that a small document does not grow it, or only once.
		keys: Vec::with_capacity(64),
		text: String::new(),
		failed: false,
	};

	value
		.serialize(ValueSerializer {
			writer: &mut writer,
			indentation: 0,
			depth: 0,
			is_root: true,
		})
		.ok()?;

	if writer.failed {
		return None;
	}

	// Each line feed is written before the line it starts, so the last line needs one, unless the tree wrote the document.
	if !writer.output.ends_with('\n') {
		writer.output.push('\n');
	}

	Some(writer.output)
}

struct Writer {
	output: String,
	/**
	Where the key of each member of the objects that are being written is in `output`, the innermost object's last, to find a duplicate key.
	*/
	keys: Vec<Range<usize>>,
	/**
	The text of a map key, or of an instant or a duration, before it is written. A key is written before its value is serialized, so one string is enough.
	*/
	text: String,
	/**
	Whether a step failed. A type can catch the error and go on, and the text then has the part that was written before the error, so the document is written again through the tree, which does not keep that part.
	*/
	failed: bool,
}

impl Writer {
	/**
	Remembers a failure in `result`, which goes back to a type's own code, which may catch it.
	*/
	fn watch(&mut self, result: Result<(), Error>) -> Result<(), Error> {
		if result.is_err() {
			self.failed = true;
		}

		result
	}

	/**
	Puts the text of a map key, or of an instant or a duration, in `text`.
	*/
	fn set_text<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
		self.text.clear();
		value.serialize(KeySerializer(&mut self.text))
	}
}

/**
The keys of one object that is being written, to find a duplicate key, as `parse::ObjectKeys` does for a reader. A key is the text that was written for it, which is a different text for each key, so the keys are compared in the output, and the keys of an object that is not large are not copied.
*/
struct WrittenKeys {
	/**
	Where the keys of this object start in `Writer::keys`.
	*/
	start: usize,
	/**
	A bit for the fingerprint of each key, as in `parse::ObjectKeys`.
	*/
	filter: u64,
	/**
	The keys, once the object has too many to compare one at a time. They are no longer in `Writer::keys` then.
	*/
	index: Option<HashSet<String>>,
}

impl WrittenKeys {
	fn new(stack: &[Range<usize>]) -> Self {
		Self {
			start: stack.len(),
			filter: 0,
			index: None,
		}
	}

	/**
	Remembers the key that was written at `key` in `output`, and returns `false` when the object has it already.
	*/
	fn insert(&mut self, stack: &mut Vec<Range<usize>>, output: &str, key: Range<usize>) -> bool {
		if let Some(index) = &mut self.index {
			return index.insert(output[key].to_owned());
		}

		let bytes = output.as_bytes();
		let bit = 1 << fingerprint(&bytes[key.clone()]);

		// The lengths and the first bytes are compared before the rest, which avoids most calls to compare memory. A written key is never empty, because an empty key is written as `''`.
		if self.filter & bit != 0
			&& stack[self.start..].iter().any(|other| {
				other.len() == key.len()
					&& bytes[other.start] == bytes[key.start]
					&& bytes[other.clone()] == bytes[key.clone()]
			}) {
			return false;
		}

		self.filter |= bit;
		stack.push(key);

		if stack.len() - self.start > MAX_FILTERED_KEYS {
			self.index = Some(
				stack
					.drain(self.start..)
					.map(|other| output[other].to_owned())
					.collect(),
			);
		}

		true
	}

	fn end(&self, stack: &mut Vec<Range<usize>>) {
		stack.truncate(self.start);
	}
}

/**
Writes a value where a value goes: at the top of the document, after a key, or as an array item.
*/
struct ValueSerializer<'a> {
	writer: &'a mut Writer,
	/**
	The indentation of the value, as `write::write_value` takes it: the lines inside a collection here get one more tab.
	*/
	indentation: usize,
	/**
	The number of collections around the value.
	*/
	depth: usize,
	/**
	Whether the value is the document itself, which must be an object or an array, and whose object has no braces.
	*/
	is_root: bool,
}

impl<'a> ValueSerializer<'a> {
	fn nested_depth(&self) -> Result<usize, Error> {
		Ok(NodeSerializer { depth: self.depth }.nested()?.depth)
	}

	/**
	A scalar, which is not a document.
	*/
	fn scalar(&mut self) -> Result<&mut String, Error> {
		if self.is_root {
			return Err(Error::write("A document must be an object or an array"));
		}

		Ok(&mut self.writer.output)
	}

	fn string(mut self, text: &str) -> Result<(), Error> {
		let output = self.scalar()?;

		// A carriage return needs escapes, so the one scan finds it too.
		if write::needs_escapes(text) {
			check_representable(text, "string")?;
			write::write_escaped_string(output, text).expect("writing to a String does not fail");
		} else {
			write::write_literal_string(output, text);
		}

		Ok(())
	}

	/**
	The text of an `Instant` or a `Duration`, which are the only types that use its private name, and which give their canonical form. It is written as it is, once `parse` accepts it, because writing the parsed value again costs about a tenth of the time to write a document with many instants. Other text that a type gives under the private name is written as it is too, which the tree serializer would put in canonical form.
	*/
	fn token<T: ?Sized + Serialize, V>(
		mut self,
		value: &T,
		parse: fn(&str) -> Result<V, String>,
	) -> Result<(), Error> {
		self.writer.set_text(value)?;
		parse(&self.writer.text).map_err(Error::write)?;
		// Only the check that this is not the document is needed from `scalar`, because the text comes from another field of the writer.
		self.scalar()?;
		self.writer.output.push_str(&self.writer.text);
		Ok(())
	}

	/**
	Starts an object here, which has no braces at the top of the document.
	*/
	fn object(self) -> Result<Collection<'a>, Error> {
		let depth = self.nested_depth()?;

		let (inner, close) = if self.is_root {
			(0, None)
		} else {
			self.writer.output.push('{');
			(self.indentation + 1, Some('}'))
		};

		Ok(Collection::new(self.writer, inner, close, depth))
	}

	fn array(self) -> Result<Collection<'a>, Error> {
		let depth = self.nested_depth()?;
		self.writer.output.push('[');

		Ok(Collection::new(
			self.writer,
			self.indentation + 1,
			Some(']'),
			depth,
		))
	}

	/**
	Starts the one-member object `{variant: …}` of an enum variant with a value, and the collection of that value, which closes the object when it ends.
	*/
	fn variant(self, variant: &'static str, is_array: bool) -> Result<Collection<'a>, Error> {
		let mut object = self.object()?;
		object.begin_member(variant)?;

		let Collection {
			writer,
			inner,
			close,
			depth,
			keys,
			..
		} = object;

		let value = ValueSerializer {
			writer,
			indentation: inner,
			depth,
			is_root: false,
		};

		let mut collection = if is_array {
			value.array()?
		} else {
			value.object()?
		};

		collection.variant = Some(Variant { inner, close, keys });
		Ok(collection)
	}

	/**
	A value that the tree serializer builds, for a case that needs the whole value before it is written.
	*/
	fn node(self, node: &crate::parse::Node<'_>) -> Result<(), Error> {
		if self.is_root {
			self.writer.output.push_str(&write::document(node)?);
		} else {
			write::write_value(&mut self.writer.output, node, self.indentation);
		}

		Ok(())
	}
}

/**
Integer types, which are an error only outside the range of a SOML int.
*/
macro_rules! serialize_integers {
	($($method:ident: $type:ty),*) => {
		$(
			fn $method(mut self, value: $type) -> Result<(), Error> {
				let value = integer(value)?;
				write::write_int(self.scalar()?, value);
				Ok(())
			}
		)*
	};
}

impl<'a> ser::Serializer for ValueSerializer<'a> {
	type Ok = ();
	type Error = Error;
	type SerializeSeq = Collection<'a>;
	type SerializeTuple = Collection<'a>;
	type SerializeTupleStruct = Collection<'a>;
	type SerializeTupleVariant = Collection<'a>;
	type SerializeMap = Collection<'a>;
	type SerializeStruct = StructSerializer<'a>;
	type SerializeStructVariant = Collection<'a>;

	fn serialize_bool(mut self, value: bool) -> Result<(), Error> {
		self.scalar()?
			.push_str(if value { "true" } else { "false" });
		Ok(())
	}

	serialize_integers!(
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

	fn serialize_f32(self, value: f32) -> Result<(), Error> {
		self.serialize_f64(widen(value))
	}

	fn serialize_f64(mut self, value: f64) -> Result<(), Error> {
		if value.is_nan() {
			return Err(Error::write("NaN is not a SOML value"));
		}

		write::write_float(self.scalar()?, value);
		Ok(())
	}

	fn serialize_char(self, value: char) -> Result<(), Error> {
		self.string(value.encode_utf8(&mut [0; 4]))
	}

	fn serialize_str(self, value: &str) -> Result<(), Error> {
		self.string(value)
	}

	fn serialize_bytes(self, value: &[u8]) -> Result<(), Error> {
		let mut array = self.array()?;

		for byte in value {
			array.begin_item();
			write::write_int(&mut array.writer.output, (*byte).into());
		}

		array.finish();
		Ok(())
	}

	fn serialize_none(mut self) -> Result<(), Error> {
		self.scalar()?.push_str("null");
		Ok(())
	}

	fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<(), Error> {
		value.serialize(self)
	}

	fn serialize_unit(self) -> Result<(), Error> {
		self.serialize_none()
	}

	fn serialize_unit_struct(self, _name: &'static str) -> Result<(), Error> {
		self.serialize_none()
	}

	fn serialize_unit_variant(
		self,
		_name: &'static str,
		_index: u32,
		variant: &'static str,
	) -> Result<(), Error> {
		self.string(variant)
	}

	fn serialize_newtype_struct<T: ?Sized + Serialize>(
		self,
		name: &'static str,
		value: &T,
	) -> Result<(), Error> {
		// `Instant` and `Duration` give their canonical text under a private name, which is written as the native value, as the tree serializer does.
		match name {
			crate::instant::TOKEN => self.token(value, Instant::parse),
			crate::duration::TOKEN => self.token(value, Duration::parse),
			_ => value.serialize(self),
		}
	}

	fn serialize_newtype_variant<T: ?Sized + Serialize>(
		self,
		_name: &'static str,
		_index: u32,
		variant: &'static str,
		value: &T,
	) -> Result<(), Error> {
		let mut object = self.object()?;
		object.member(variant, value)?;
		object.finish();
		Ok(())
	}

	fn serialize_seq(self, _length: Option<usize>) -> Result<Collection<'a>, Error> {
		self.array()
	}

	fn serialize_tuple(self, _length: usize) -> Result<Collection<'a>, Error> {
		self.array()
	}

	fn serialize_tuple_struct(
		self,
		_name: &'static str,
		_length: usize,
	) -> Result<Collection<'a>, Error> {
		self.array()
	}

	fn serialize_tuple_variant(
		self,
		_name: &'static str,
		_index: u32,
		variant: &'static str,
		_length: usize,
	) -> Result<Collection<'a>, Error> {
		self.variant(variant, true)
	}

	fn serialize_map(self, _length: Option<usize>) -> Result<Collection<'a>, Error> {
		self.object()
	}

	fn serialize_struct(
		self,
		name: &'static str,
		length: usize,
	) -> Result<StructSerializer<'a>, Error> {
		// A `std::time::Duration` is found by its shape after its fields, so the tree serializer builds a struct with its name.
		if name == "Duration" {
			let tree = ser::Serializer::serialize_struct(
				NodeSerializer { depth: self.depth },
				name,
				length,
			)?;
			return Ok(StructSerializer::Tree(tree, self));
		}

		self.object().map(StructSerializer::Stream)
	}

	fn serialize_struct_variant(
		self,
		_name: &'static str,
		_index: u32,
		variant: &'static str,
		_length: usize,
	) -> Result<Collection<'a>, Error> {
		self.variant(variant, false)
	}
}

/**
The one-member object around the collection of an enum variant: the tabs of its member, its closing brace, which a top-level object does not have, and its key.
*/
struct Variant {
	inner: usize,
	close: Option<char>,
	keys: WrittenKeys,
}

/**
An array or an object that is being written.
*/
struct Collection<'a> {
	writer: &'a mut Writer,
	/**
	The tabs before each item or member, which is also the indentation of its value.
	*/
	inner: usize,
	/**
	The closing bracket, or `None` for a top-level object, which has no braces.
	*/
	close: Option<char>,
	count: usize,
	/**
	The number of collections around the items or members.
	*/
	depth: usize,
	keys: WrittenKeys,
	/**
	Whether `Writer::text` holds the key of the next member of a map.
	*/
	has_key: bool,
	variant: Option<Variant>,
}

impl<'a> Collection<'a> {
	fn new(writer: &'a mut Writer, inner: usize, close: Option<char>, depth: usize) -> Self {
		Self {
			keys: WrittenKeys::new(&writer.keys),
			writer,
			inner,
			close,
			count: 0,
			depth,
			has_key: false,
			variant: None,
		}
	}

	/**
	Starts the line of a new item or member. The first member of a top-level object starts the document, so it has no line feed before it.
	*/
	fn begin_item(&mut self) {
		if self.close.is_some() || self.count > 0 {
			write::push_line(&mut self.writer.output, self.inner);
		}

		self.count += 1;
	}

	/**
	Writes the key of a new member, and fails when the key cannot be written or the object already has it.
	*/
	fn begin_member(&mut self, key: &str) -> Result<(), Error> {
		self.begin_item();
		let start = self.writer.output.len();

		// A bare key has no carriage return, so only a quoted one is checked.
		if is_bare_key(key) {
			self.writer.output.push_str(key);
		} else {
			check_representable(key, "key")?;
			write::write_string(&mut self.writer.output, key)
				.expect("writing to a String does not fail");
		}

		let end = self.writer.output.len();

		// Two pushes, which are inlined, rather than a copy of two bytes.
		self.writer.output.push(':');
		self.writer.output.push(' ');

		// The keys are compared in the output, so the key is written before it is checked. The text is thrown away when this fails.
		if !self
			.keys
			.insert(&mut self.writer.keys, &self.writer.output, start..end)
		{
			return Err(Error::write("Duplicate key"));
		}

		Ok(())
	}

	fn value(&mut self) -> ValueSerializer<'_> {
		ValueSerializer {
			writer: &mut *self.writer,
			indentation: self.inner,
			depth: self.depth,
			is_root: false,
		}
	}

	fn item<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
		self.begin_item();
		let result = value.serialize(self.value());
		self.writer.watch(result)
	}

	fn member<T: ?Sized + Serialize>(&mut self, key: &str, value: &T) -> Result<(), Error> {
		let result = self
			.begin_member(key)
			.and_then(|()| value.serialize(self.value()));
		self.writer.watch(result)
	}

	fn finish(self) {
		close(&mut self.writer.output, self.inner, self.close, self.count);
		self.keys.end(&mut self.writer.keys);

		if let Some(variant) = self.variant {
			close(&mut self.writer.output, variant.inner, variant.close, 1);
			variant.keys.end(&mut self.writer.keys);
		}
	}
}

/**
Writes the end of a collection that has `count` items or members.
*/
fn close(output: &mut String, inner: usize, bracket: Option<char>, count: usize) {
	match bracket {
		// An empty top-level object keeps its braces, because an empty document is not valid.
		None if count == 0 => output.push_str("{}"),
		None => {}
		Some(bracket) => {
			if count > 0 {
				write::push_line(output, inner - 1);
			}

			output.push(bracket);
		}
	}
}

macro_rules! serialize_items {
	($($trait:ident::$method:ident),*) => {
		$(
			impl ser::$trait for Collection<'_> {
				type Ok = ();
				type Error = Error;

				fn $method<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
					self.item(value)
				}

				fn end(self) -> Result<(), Error> {
					self.finish();
					Ok(())
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

impl ser::SerializeMap for Collection<'_> {
	type Ok = ();
	type Error = Error;

	fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Error> {
		let result = self.writer.set_text(key);
		// A key that fails does not take the place of the key before it, as in the tree serializer.
		self.has_key |= result.is_ok();
		self.writer.watch(result)
	}

	fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
		assert!(
			std::mem::take(&mut self.has_key),
			"serde calls serialize_key before serialize_value"
		);

		// The key is taken out while it is written, and put back before the value is serialized, so that the value can use the string for its own keys.
		let key = std::mem::take(&mut self.writer.text);
		let result = self.begin_member(&key);
		self.writer.text = key;

		let result = result.and_then(|()| value.serialize(self.value()));
		self.writer.watch(result)
	}

	fn end(self) -> Result<(), Error> {
		self.finish();
		Ok(())
	}
}

impl ser::SerializeStructVariant for Collection<'_> {
	type Ok = ();
	type Error = Error;

	fn serialize_field<T: ?Sized + Serialize>(
		&mut self,
		key: &'static str,
		value: &T,
	) -> Result<(), Error> {
		self.member(key, value)
	}

	fn end(self) -> Result<(), Error> {
		self.finish();
		Ok(())
	}
}

/**
A struct, which the tree serializer builds when it may be a `std::time::Duration`.
*/
enum StructSerializer<'a> {
	Stream(Collection<'a>),
	Tree(super::MapSerializer, ValueSerializer<'a>),
}

impl ser::SerializeStruct for StructSerializer<'_> {
	type Ok = ();
	type Error = Error;

	fn serialize_field<T: ?Sized + Serialize>(
		&mut self,
		key: &'static str,
		value: &T,
	) -> Result<(), Error> {
		match self {
			Self::Stream(object) => object.member(key, value),
			Self::Tree(tree, _) => ser::SerializeStruct::serialize_field(tree, key, value),
		}
	}

	fn end(self) -> Result<(), Error> {
		match self {
			Self::Stream(object) => {
				object.finish();
				Ok(())
			}
			Self::Tree(tree, place) => place.node(&tree.finish()?),
		}
	}
}
