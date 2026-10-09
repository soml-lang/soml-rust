use crate::parse::MAX_DEPTH;
use crate::tree::{self, Comment, Decor, Entries, Entry, Item, Key, Member, Node, Object, Printer};
use crate::{Error, Value};
use std::borrow::Cow;
use std::fmt::{self, Display, Write as _};
use std::str::FromStr;

/**
A SOML document as a lossless syntax tree, to read with positions, change, and print.

An unchanged document prints the exact text it was parsed from. A changed one keeps the comments, blank lines, indentation, member order, and the author's spelling of every value outside the change. New values are written in canonical style, and on one line in a container that is on one line.

```rust
let mut document: soml::Document = "
## The edge service.
name: 'api-gateway'
port: 0x1F90 # The default.
".parse()?;

document.set(["port"], 8080)?;
document.set(["replicas"], 3)?;
document.remove(["name"])?;

assert_eq!(document.to_string(), "
## The edge service.
port: 8080 # The default.
replicas: 3
");
# Ok::<(), soml::Error>(())
```

See [`tree`](crate::tree) for the nodes and how whitespace and comments are kept.
*/
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
	/**
	The text before and after a top-level collection written with braces.
	*/
	outer: Decor,
	root: Node,
	source: String,
}

/**
One step of a path into a document: a key of an object, or a position in an array.
*/
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathSegment<'a> {
	/**
	A key of an object.
	*/
	Key(&'a str),
	/**
	A position in an array.
	*/
	Index(usize),
}

impl<'a> From<&'a str> for PathSegment<'a> {
	fn from(key: &'a str) -> Self {
		Self::Key(key)
	}
}

impl<'a> From<&'a String> for PathSegment<'a> {
	fn from(key: &'a String) -> Self {
		Self::Key(key)
	}
}

impl From<usize> for PathSegment<'_> {
	fn from(index: usize) -> Self {
		Self::Index(index)
	}
}

impl Display for PathSegment<'_> {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Key(key) => crate::write::write_key(formatter, key),
			Self::Index(index) => write!(formatter, "[{index}]"),
		}
	}
}

/**
The segments of a path given to `set` or `remove`, which must have at least one.
*/
fn path_segments<'a>(
	path: impl IntoIterator<Item = impl Into<PathSegment<'a>>>,
) -> Result<Vec<PathSegment<'a>>, Error> {
	let path: Vec<PathSegment<'a>> = path.into_iter().map(Into::into).collect();

	if path.is_empty() {
		return Err(Error::data(
			"The path must be a non-empty array of keys and array indexes",
		));
	}

	Ok(path)
}

/**
The error for a path that does not fit the document, in the words of the JS reference implementation, such as `Cannot edit a.b, because a is not an object or an array`, where `reason` is about the value that holds `path[index]`.
*/
#[cold]
fn path_error(path: &[PathSegment<'_>], index: usize, reason: &str) -> Error {
	Error::data(format!(
		"Cannot edit {}, because {} {reason}",
		describe_path(path),
		describe_parent(path, index)
	))
}

/**
A path for an error message, as the JS reference implementation writes it, such as `servers[0]."the name"`: each key bare when it can be and otherwise as a JSON string, and each index in brackets. The path may come from user input, so it is cut to 200 characters, and the invisible characters and those that a terminal acts on are written as escapes, such as `\u{202e}`.
*/
fn describe_path(path: &[PathSegment<'_>]) -> String {
	let mut text = String::new();

	for (position, segment) in path.iter().enumerate() {
		match segment {
			PathSegment::Key(key) => {
				if position > 0 {
					text.push('.');
				}

				crate::parse::write_described_key(&mut text, key);
			}
			PathSegment::Index(index) => {
				write!(text, "[{index}]").expect("writing to a String does not fail");
			}
		}
	}

	let mut description = String::new();

	for character in crate::abbreviate(&text, 200).chars() {
		// The control, format, line separator, and paragraph separator characters, which a JSON string leaves as they are.
		if character.is_control()
			|| matches!(character, '\u{2028}' | '\u{2029}')
			|| is_format_character(character)
		{
			write!(description, "\\u{{{:x}}}", u32::from(character))
				.expect("writing to a String does not fail");
		} else {
			description.push(character);
		}
	}

	description
}

/**
The value that holds `path[index]`, for an error message.
*/
fn describe_parent(path: &[PathSegment<'_>], index: usize) -> String {
	if index == 0 {
		"the document".to_owned()
	} else {
		describe_path(&path[..index])
	}
}

/**
Whether a character is in the Unicode general category Format, as of Unicode 17.0, the version of Node.js 26, which the JS reference implementation escapes in a path.
*/
const fn is_format_character(character: char) -> bool {
	matches!(
		character,
		'\u{AD}'
			| '\u{600}'..='\u{605}'
			| '\u{61C}'
			| '\u{6DD}'
			| '\u{70F}'
			| '\u{890}'..='\u{891}'
			| '\u{8E2}'
			| '\u{180E}'
			| '\u{200B}'..='\u{200F}'
			| '\u{202A}'..='\u{202E}'
			| '\u{2060}'..='\u{2064}'
			| '\u{2066}'..='\u{206F}'
			| '\u{FEFF}'
			| '\u{FFF9}'..='\u{FFFB}'
			| '\u{110BD}'
			| '\u{110CD}'
			| '\u{13430}'..='\u{1343F}'
			| '\u{1BCA0}'..='\u{1BCA3}'
			| '\u{1D173}'..='\u{1D17A}'
			| '\u{E0001}'
			| '\u{E0020}'..='\u{E007F}'
	)
}

/**
The indentation of the line an item starts on: the spaces and tabs after the last line break before it, or the indentation of its container's line when there is no line break.
*/
fn line_indentation(separator: &str, inherited: &str) -> String {
	match last_line_start(separator) {
		Some(start) => tree::indentation_of(&separator[start..]).to_owned(),
		None => inherited.to_owned(),
	}
}

/**
The comment lines and blank lines of leading decor, without the indentation of the item's own line.
*/
fn comment_lines(leading: &str) -> &str {
	last_line_start(leading).map_or("", |start| &leading[..start])
}

/**
The indentation of the item's own line in leading decor.
*/
fn own_line(leading: &str) -> &str {
	last_line_start(leading).map_or(leading, |start| &leading[start..])
}

/**
The index after the last line feed in decor that is not inside a block comment, which is where the last line starts. A block comment that spans lines is part of the line it ends on.
*/
fn last_line_start(decor: &str) -> Option<usize> {
	tree::decor_parts(decor)
		.filter(|(part, _)| *part == tree::DecorPart::LineFeed)
		.last()
		.map(|(_, range)| range.end)
}

/**
The error for a path segment at `offset` in `path` that does not fit `node`: an index for an object, a key for an array, or any segment for a scalar.
*/
#[cold]
fn segment_error(node: &Node, path: &[PathSegment<'_>], offset: usize) -> Error {
	let reason = match node {
		Node::Object(_) => "is an object, so it needs a key, not an index",
		Node::Array(_) => "is an array, so it needs an index, not a key",
		Node::Scalar(_) => "is not an object or an array",
	};

	path_error(path, offset, reason)
}

/**
The position of the member or item that a path segment names in a node.
*/
fn step(node: &Node, segment: PathSegment<'_>) -> Option<usize> {
	match (node, segment) {
		(Node::Object(object), PathSegment::Key(key)) => object.position(key),
		(Node::Array(array), PathSegment::Index(index)) if index < array.items().len() => {
			Some(index)
		}
		_ => None,
	}
}

impl Document {
	/**
	The top-level object or array.
	*/
	#[must_use]
	pub const fn root(&self) -> &Node {
		&self.root
	}

	/**
	The top-level object or array, to change. Changes here can make a document that is not valid, such as one with two members with the same key; [`to_value`](Self::to_value) finds that. [`set`](Self::set) and [`remove`](Self::remove) keep the document valid.
	*/
	pub const fn root_mut(&mut self) -> &mut Node {
		&mut self.root
	}

	/**
	The text before a top-level collection written with braces, as its leading decor, and the text after it, as its trailing decor. Both are empty for a top-level object without braces, where that text belongs to its first member and to the object itself.
	*/
	#[must_use]
	pub const fn outer_decor(&self) -> &Decor {
		&self.outer
	}

	/**
	The node at a path. `None` when nothing is there.

	```rust
	let document: soml::Document = "server: {port: 8080}\nhosts: ['a', 'b']".parse()?;

	let port = document.get(["server", "port"]).and_then(soml::tree::Node::as_scalar);
	assert_eq!(port.map(soml::tree::Scalar::value), Some(&soml::Value::Int(8080)));

	let host = document.get([soml::PathSegment::from("hosts"), 1.into()]).and_then(soml::tree::Node::as_scalar);
	assert_eq!(host.and_then(soml::tree::Scalar::raw), Some("'b'"));
	# Ok::<(), soml::Error>(())
	```
	*/
	#[must_use]
	pub fn get<'a>(
		&self,
		path: impl IntoIterator<Item = impl Into<PathSegment<'a>>>,
	) -> Option<&Node> {
		let mut node = &self.root;

		for segment in path {
			let index = step(node, segment.into())?;

			node = match node {
				Node::Object(object) => object.members()[index].value(),
				Node::Array(array) => array.items()[index].value(),
				Node::Scalar(_) => unreachable!("a path does not step into a scalar"),
			};
		}

		Some(node)
	}

	/**
	The node at a path, to change. See [`get`](Self::get).
	*/
	pub fn get_mut<'a>(
		&mut self,
		path: impl IntoIterator<Item = impl Into<PathSegment<'a>>>,
	) -> Option<&mut Node> {
		let mut node = &mut self.root;

		for segment in path {
			let index = step(node, segment.into())?;

			node = match node {
				Node::Object(object) => object.members_mut()[index].value_mut(),
				Node::Array(array) => array.items_mut()[index].value_mut(),
				Node::Scalar(_) => unreachable!("a path does not step into a scalar"),
			};
		}

		Some(node)
	}

	/**
	Sets the value at a path, replacing what is there or adding it.

	- An existing value is replaced, and the comments around it stay. A value that replaces a block string on the line after its key goes on the key's line, as the formatter writes it, unless a comment comes between them.
	- A new member goes after the last member of its object, and after the comments that member owns (see [`remove`](Self::remove)), so `[1, 2 /* note */]` with a new item `3` becomes `[1, 2 /* note */, 3]`. When something follows that member on its line, such as the closing bracket, the new one goes on that line, after a comma. Otherwise, it goes on a line of its own, with the indentation of that member and no comma, because a line break separates it.
	- A missing parent object is created with braces.
	- An index equal to an array's length appends. The keys after it in the path are new objects in the new item, so setting `["b", 0, "c"]` to `1` in `b: []` adds the item `{c: 1}`.
	- A new value is written in canonical style, on lines of its own. In a container that stays on one line, it is written on one line, so `[1, 2]` with a new item `{a: 3}` becomes `[1, 2, {a: 3}]`. A container stays on one line when its brackets are on one line and something other than spaces and tabs is between them, as in `[1, 2]` and `[/* note */]`, or when it is inside a container that stays on one line.
	- In a container with no entries, a new entry goes on a line of its own, so `deps: {}` gets its first member on a line of its own, because the formatter writes every empty container as `{}`. In a container that stays on one line, it goes before the closing bracket instead, so `[/* note */]` becomes `[/* note */ 1]`, and `a: [1, []]` becomes `a: [1, [2]]`.

	A change always gives a valid document with the expected value, and it keeps every comment except those inside a value it replaces and those that a member or an item it removes owns. These are the spec's editing rules, as in the JavaScript reference, so a change to a formatted document gives a formatted document, with the same text as there. A comment on a line of its own before a closing bracket belongs to no member or item, so a new entry goes before it, and it stays when the last entry is removed.

	# Errors

	Returns an error of kind [`ErrorKind::Data`](crate::ErrorKind::Data) when the path is empty, when it goes through a scalar, when it uses a key on an array or an index on an object, when an index is past the end of its array, so that the item before the new one would be missing, or when an index is below a value that does not exist, because a missing value is made as an object. Returns an error of kind [`ErrorKind::Write`](crate::ErrorKind::Write) when the value holds something SOML cannot represent, and when the result would not be a valid document, such as one nested more than 100 levels deep. Such an error has no position, because the place it is about is not in the document. The document is unchanged after an error.

	Each change reads the result back to check it, so it takes time in proportion to the size of the document.
	*/
	pub fn set<'a>(
		&mut self,
		path: impl IntoIterator<Item = impl Into<PathSegment<'a>>>,
		value: impl Into<Value>,
	) -> Result<(), Error> {
		let path = path_segments(path)?;

		// Every key the change may write is checked up front, also one it nests in a new object, which no `Member::new` checks, so a key that cannot be written always gives the same error.
		for segment in &path {
			if let PathSegment::Key(key) = segment {
				crate::write::check_representable(key, "key")?;
			}
		}

		let value = tree::checked(value.into())?;
		let before = self.root.clone();
		// A document indented like the code around it, as in `\t{}`, gets new lines at the indentation of its top-level collection's line. The outer decor is empty for a top-level object without braces.
		let indentation = tree::indentation_of(own_line(&self.outer.leading)).to_owned();
		let result = set_in(&mut self.root, &path, &path, value, &indentation, false);

		// Every rule of the spec is checked by reading the result back, such as the nesting limit at the place the value goes, and a key that cannot be written. A failed change leaves the document as it was. The error has no position, because its position is in the text the change would have made, which nobody sees.
		if let Err(error) = result.and_then(|()| {
			self.to_value()
				.map(|_| ())
				.map_err(|error| Error::write(error.message()))
		}) {
			self.root = before;
			return Err(error);
		}

		Ok(())
	}

	/**
	Removes the member or item at a path, with the comments it owns and its comma. A member or an item owns the comments that the formatter keeps with it: the comments after it on its line, before its comma, or after its comma when nothing else follows on that line, and the block comments before it on its line, back to the comma or bracket before it. So in `[1 /* a */, /* b */ 2 /* c */]`, `1` owns `/* a */`, and `2` owns `/* b */` and `/* c */`. A comment on a line of its own belongs to no member or item, so the comment lines above and below a removed one stay.

	The other entries keep their commas, except that when the removed entries are the last ones and the last of them has no comma after it, the comma directly before them goes too, when only spaces, tabs, and the comments the first of them owns are between, so `[1, 2]` and `[1, /* note */ 2]` become `[1]`, and `[\n\t1,\n\t2\n]` becomes `[\n\t1,\n]`. A blank line the removal leaves next to another blank line, or directly inside a bracket or at the start or end of the document, goes too, the one before it when there is a choice, so a formatted document stays formatted. This takes at most one blank line, so other blank lines stay as written.

	Removing every entry of a container closes it up to `[]` or `{}`, unless a comment on a line of its own is left inside. Removing the only member of a top-level object without braces leaves `{}` in its place, because a document cannot be empty, and members set later go inside the braces. The `{}` keeps the comments that the member owned, so `a: 1 # note` without `a` becomes `{} # note`.

	These are the spec's editing rules, as in the JavaScript reference. See [`set`](Self::set) for which layouts are kept exactly.

	Removing a value that does not exist changes nothing and is not an error: a missing member, a missing object or array on the way, or an index at or past the end of its array, and anything below them. So removing the same path twice is safe. Returns whether something was removed, so a caller who expected a value can notice that it was missing.

	# Errors

	Returns an error of kind [`ErrorKind::Data`](crate::ErrorKind::Data) when the path is empty, when it goes through a scalar, or when it uses a key on an array or an index on an object, before it reaches a value that does not exist. The document is unchanged after an error.
	*/
	pub fn remove<'a>(
		&mut self,
		path: impl IntoIterator<Item = impl Into<PathSegment<'a>>>,
	) -> Result<bool, Error> {
		let path = path_segments(path)?;

		// A top-level object without braces needs a member, so when its only member goes, `{}` takes its place, with its decor, and members set later go inside the braces, as in the JavaScript reference.
		if let Node::Object(object) = &mut self.root
			&& !object.is_braced()
			&& let [member] = object.members()
			&& [PathSegment::Key(member.key().value())] == path.as_slice()
		{
			let member = object.members_mut().remove(0);
			self.outer = Decor {
				leading: member.decor().leading.clone(),
				trailing: format!("{}{}", member.decor().trailing, object.closing_decor()),
			};
			self.root = Node::Object(Object::default());
			return Ok(true);
		}

		// `remove_in` fails only before it changes anything, so unlike `set`, this keeps no copy to restore.
		remove_in(&mut self.root, &path, &path)
	}

	/**
	The comments, in order, with their positions in the document's current text, which is the parsed text when nothing changed.

	After a change, these spans do not fit [`position`](Self::position), which uses the parsed text. Print the document and parse it again to get both for the new text.
	*/
	#[must_use]
	pub fn comments(&self) -> Vec<Comment> {
		let mut printer = Printer::new(true);

		printer.document(&self.outer, &self.root);
		printer.comments.unwrap_or_default()
	}

	/**
	The line and column of a byte offset in the text the document was parsed from, as for a node's span.

	```rust
	let document: soml::Document = "name: 'a'\nport: 80".parse()?;
	let span = document.get(["port"]).and_then(soml::tree::Node::span).unwrap();

	assert_eq!(document.position(span.start), soml::LineColumn { line: 2, column: 7 });
	# Ok::<(), soml::Error>(())
	```
	*/
	#[must_use]
	pub fn position(&self, offset: usize) -> crate::LineColumn {
		crate::LineColumn::locate(&self.source, offset)
	}

	/**
	The document in the layout of the spec's formatter, which changes layout and nothing else.

	It keeps the comments where they are, the member order, the spelling of every key and value, block strings, and a top level with or without braces. It writes one tab of indentation per level, every member and item on its own line, no commas, at most one blank line in a row, no trailing spaces or tabs, and one line feed at the end. An object or an array whose brackets are on one line stays on one line, with a comma and a space between its members or items. To give it one member or item per line, put a line break anywhere inside it. Formatting a formatted document gives the same text.

	```rust
	let document: soml::Document = "# Server\nserver: {host: 'a',   port: 0x50,} # Main.\n\n\nlist: [1,\n2]".parse()?;

	assert_eq!(document.format(), "# Server\nserver: {host: 'a', port: 0x50} # Main.\n\nlist: [\n\t1\n\t2\n]\n");
	# Ok::<(), soml::Error>(())
	```

	The spec's formatter rules are normative, so every conforming formatter gives the same text. The [`tree`](crate::tree) module lists them in detail.
	*/
	#[must_use]
	pub fn format(&self) -> String {
		tree::format::document(&self.outer, &self.root)
	}

	/**
	The value of the document, with every check of the spec, such as for duplicate keys, which a change through [`root_mut`](Self::root_mut) can make.

	It prints the document and reads it back, so an error's position refers to the printed text, which is the parsed text when nothing changed.

	# Errors

	Returns an error when the document, as changed, is not valid.
	*/
	pub fn to_value(&self) -> Result<Value, Error> {
		self.to_string().parse()
	}
}

/**
Whether a container stays on one line when a change adds or replaces a value in it: its brackets are on one line, and something other than spaces and tabs is between them, which the formatter keeps on one line. An empty `[]` or `{}` says nothing about layout, because the formatter writes every empty container that way. A top-level object without braces never does.
*/
fn stays_on_one_line(node: &Node) -> bool {
	let has_content = |entry_count: usize, closing: &str| {
		entry_count > 0 || !closing.trim_matches([' ', '\t']).is_empty()
	};

	let is_braced_with_content = match node {
		Node::Object(object) => {
			object.is_braced() && has_content(object.members().len(), object.closing_decor())
		}
		Node::Array(array) => has_content(array.items().len(), array.closing_decor()),
		Node::Scalar(_) => false,
	};

	is_braced_with_content && node.is_on_one_line()
}

/**
A node for a new value: laid out on lines of its own at `indentation`, as in canonical form, or on one line in a container that stays on one line.
*/
fn new_node(value: Value, indentation: &str, is_one_line: bool) -> Node {
	if is_one_line {
		Node::from_value_on_one_line(value)
	} else {
		Node::from_value(value, indentation)
	}
}

/**
`value` inside new objects for the keys of `path` from `start` on, with braces, as canonical form writes a missing parent. An index there names an item of an array that does not exist yet, so it is an error, which names the last such index, as in the JavaScript reference.
*/
fn nest(path: &[PathSegment<'_>], start: usize, value: Value) -> Result<Value, Error> {
	// The index error comes before the depth check, so that it names the index however long the path is, and before any object is made, because dropping a deep value could overflow the stack.
	if let Some((position, index)) =
		path.iter()
			.enumerate()
			.skip(start)
			.rev()
			.find_map(|(position, segment)| match segment {
				PathSegment::Index(index) => Some((position, index)),
				PathSegment::Key(_) => None,
			}) {
		return Err(path_error(
			path,
			position,
			&format!("does not exist, so it has no index {index}"),
		));
	}

	// The value goes inside one collection for each segment of the path, so a path longer than the nesting limit can only give a document that no reader accepts, and building its objects one inside the other could overflow the stack.
	if path.len() > MAX_DEPTH {
		return Err(Error::write(format!(
			"The document is nested more than {MAX_DEPTH} levels deep"
		)));
	}

	Ok(path[start..]
		.iter()
		.rev()
		.fold(value, |value, segment| match segment {
			PathSegment::Key(key) => {
				Value::Object(crate::Object::from([((*key).to_owned(), value)]))
			}
			PathSegment::Index(_) => unreachable!("an index is an error above"),
		}))
}

/**
Sets `value` at `path` inside `node`, whose line has `indentation`. `is_inside_one_line` is whether `node` is inside a container that stays on one line, which keeps it on one line too. `full_path` is for error messages.
*/
fn set_in(
	node: &mut Node,
	path: &[PathSegment<'_>],
	full_path: &[PathSegment<'_>],
	value: Value,
	indentation: &str,
	is_inside_one_line: bool,
) -> Result<(), Error> {
	// The end of the path, where the value replaces `node`. It is on one line inside a container that stays on one line.
	if path.is_empty() {
		*node = new_node(value, indentation, is_inside_one_line);
		return Ok(());
	}

	let is_one_line = is_inside_one_line || stays_on_one_line(node);
	// Where `path` starts in `full_path`.
	let offset = full_path.len() - path.len();

	match (node, path[0]) {
		(Node::Object(object), PathSegment::Key(key)) => set_in_object(
			object,
			key,
			path,
			full_path,
			value,
			indentation,
			is_one_line,
		),
		(Node::Array(array), PathSegment::Index(index)) => {
			let count = array.items().len();

			if index > count {
				let owner = if offset == 0 {
					"the document".to_owned()
				} else {
					format!("the array at {}", describe_path(&full_path[..offset]))
				};
				let items = if count == 1 {
					"1 item".to_owned()
				} else {
					format!("{count} items")
				};
				return Err(Error::data(format!(
					"Cannot edit {}, because {owner} has {items}. Add an item at index {count}",
					describe_path(full_path)
				)));
			}

			// An index equal to the length appends, and the rest of the path is new objects in the new item, as in the JavaScript reference.
			if index == count {
				let value = nest(full_path, offset + 1, value)?;
				// The null is a placeholder: `insert` puts the new value in its place, laid out for its line.
				let item = Item::new(Value::Null).expect("null can be written");
				array
					.entries_mut()
					.insert(count, item, value, indentation, is_one_line);
				return Ok(());
			}

			let item_indentation = entry_indentation(array.items(), index, false, indentation);

			set_in(
				array.items_mut()[index].value_mut(),
				&path[1..],
				full_path,
				value,
				&item_indentation,
				is_one_line,
			)
		}
		(node, _) => Err(segment_error(node, full_path, offset)),
	}
}

/**
Sets `value` at `path` inside `object`, where `key` is the first segment of `path`.
*/
fn set_in_object(
	object: &mut Object,
	key: &str,
	path: &[PathSegment<'_>],
	full_path: &[PathSegment<'_>],
	value: Value,
	indentation: &str,
	is_one_line: bool,
) -> Result<(), Error> {
	let offset = full_path.len() - path.len();

	if let Some(index) = object.position(key) {
		let member = &mut object.members_mut()[index];

		// A block string begins on the line after its key, and the new value is never one, so it goes on the key's line, as the formatter writes it, when only whitespace is between them.
		if path.len() == 1
			&& matches!(member.value(), Node::Scalar(scalar) if scalar.is_block_string())
			&& member
				.after_colon
				.bytes()
				.all(|byte| matches!(byte, b' ' | b'\t' | b'\n'))
		{
			member.after_colon = Cow::Borrowed(" ");
		}

		// A value on the line after its key, because a comment comes between them, is indented like its own line.
		let member_indentation = line_indentation(
			&object.members()[index].after_colon,
			&entry_indentation(object.members(), index, !object.is_braced(), indentation),
		);

		return set_in(
			object.members_mut()[index].value_mut(),
			&path[1..],
			full_path,
			value,
			&member_indentation,
			is_one_line,
		);
	}

	let value = nest(full_path, offset + 1, value)?;
	// The null is a placeholder, as for a new item.
	let member = Member::new(Key::new(key), Value::Null)?;
	let count = object.members().len();
	object
		.entries_mut()
		.insert(count, member, value, indentation, is_one_line);
	Ok(())
}

/**
The indentation of the line an entry starts on, which may be the line of an entry before it. `starts_line` is whether the first entry starts a line, which is true at the top of a document.
*/
fn entry_indentation<T: Entry>(
	entries: &[T],
	index: usize,
	starts_line: bool,
	indentation: &str,
) -> String {
	for position in (0..=index).rev() {
		let previous_trailing = match position.checked_sub(1) {
			Some(previous) => entries[previous].separator().decor.trailing.as_str(),
			None if starts_line => "\n",
			None => "",
		};
		let decor = format!(
			"{previous_trailing}{}",
			entries[position].separator().decor.leading
		);

		if last_line_start(&decor).is_some() {
			return line_indentation(&decor, indentation);
		}

		// Without a line feed in the decor, the entry is on the line where the entry before it ends.
		if let Some(indentation) = position
			.checked_sub(1)
			.and_then(|previous| last_line_indentation(&entries[previous]))
		{
			return indentation;
		}
	}

	indentation.to_owned()
}

/**
The indentation of the last line of an entry that spans lines, as an object or a block string can, or `None` for an entry on one line. A line feed in a block comment does not start a line, as in decor.
*/
fn last_line_indentation<T: Entry>(entry: &T) -> Option<String> {
	let mut printer = Printer::new(true);
	entry.print(&mut printer);
	let text = printer.output;
	// The comments are in text order, so one walk back through them finds each comment a line feed may be in.
	let mut comments = printer.comments.unwrap_or_default().into_iter().rev();
	let mut comment = comments.next();
	let mut end = text.len();

	let line_feed = loop {
		let line_feed = text[..end].rfind('\n')?;

		while comment
			.as_ref()
			.is_some_and(|comment| comment.span.start > line_feed)
		{
			comment = comments.next();
		}

		match &comment {
			Some(comment) if comment.span.contains(&line_feed) => end = comment.span.start,
			_ => break line_feed,
		}
	};

	Some(tree::indentation_of(&text[line_feed + 1..]).to_owned())
}

/**
Ends the line of the token before a removed entry that ended it: the last kept entry, whose trailing decor holds the line feed that ends its line, as the parser gives it, or an opening bracket, whose line feed starts the decor after it.
*/
fn end_line<T: Entry>(kept: &mut [T], moved: &mut String) {
	match kept.last_mut() {
		Some(last) => last.separator_mut().decor.trailing.push('\n'),
		None => moved.push('\n'),
	}
}

impl<T: Entry> Entries<'_, T> {
	/**
	Moves the line feed that ends the last entry's line from the closing decor to the entry's trailing decor, where the parser puts it. An entry pushed through the node API has no decor, so that line feed can be in the closing decor, and then the entry would be taken as followed by something on its line, and the comment lines after it as its own.
	*/
	fn end_last_line(&mut self) {
		let Some(last) = self.list.last_mut() else {
			return;
		};

		if last.separator().decor.trailing.ends_with('\n') {
			return;
		}

		let line_feed = tree::first_line_end(self.closing);

		if let Some(line_feed) = line_feed {
			last.separator_mut()
				.decor
				.trailing
				.extend(self.closing.drain(..line_feed));
		}
	}

	/**
	Inserts an entry with a value at `index`, as in the JavaScript reference. When something follows the entry before it on its line, such as the closing bracket, the new entry goes on that line after a comma, in the comma style of that entry. Otherwise, it goes on a line of its own with no comma, as a line break separates it, and so does an entry in an empty container, unless the container stays on one line. `indentation` is that of the container's line, and `is_one_line` is whether the container stays on one line, which keeps the new value on one line too.
	*/
	fn insert(
		mut self,
		index: usize,
		mut entry: T,
		value: Value,
		indentation: &str,
		is_one_line: bool,
	) {
		self.end_last_line();
		let Self {
			list: entries,
			closing,
			is_braced,
		} = self;
		let (leading, trailing, has_comma, line) = match index.checked_sub(1) {
			Some(previous) => {
				let line = entry_indentation(entries, previous, !is_braced, indentation);
				let separator = entries[previous].separator();
				let decor = &separator.decor;

				if is_braced && !decor.trailing.ends_with('\n') {
					// Something follows the entry before it on its line, such as the next entry or the closing bracket, so the new entry goes on that line, after a comma, in the comma style of that entry, as in the JavaScript reference. That line is the one the entry before it ends on.
					let line = last_line_indentation(&entries[previous]).unwrap_or(line);
					let has_comma = separator.has_comma;

					// The comments after the last entry on its line, before the closing bracket, are its own, so the new entry goes after them, and they go before the comma the entry gets.
					if index == entries.len() && !has_comma {
						let owned = closing.trim_end_matches([' ', '\t']).len();
						entries[previous]
							.separator_mut()
							.before_comma
							.extend(closing.drain(..owned));
					}

					(" ".to_owned(), String::new(), has_comma, line)
				} else if decor.trailing.ends_with('\n') {
					// On a line of its own, a line break separates the entry, so it needs no comma, whatever the other entries have.
					(line.clone(), "\n".to_owned(), false, line)
				} else {
					// The last member of a document without a final line feed. The line feed that now ends its line belongs to it, as its trailing decor, so a later removal of either member handles it. Spaces and tabs at the end of a line mean nothing, except in a line comment.
					let trailing = &mut entries[previous].separator_mut().decor.trailing;

					if !tree::ends_in_line_comment(trailing) {
						trailing.truncate(trailing.trim_end_matches([' ', '\t']).len());
					}

					trailing.push('\n');
					(line.clone(), String::new(), false, line)
				}
			}
			// Only a change through the node API can empty a top-level object without braces, because `remove` puts `{}` in place of its last member.
			None if !is_braced => (String::new(), "\n".to_owned(), false, String::new()),
			// A container that stays on one line keeps the new entry on that line, before the closing bracket, so `[/* note */]` becomes `[/* note */ 1]`, and `[1, []]` becomes `[1, [2]]`.
			None if is_one_line => {
				let before = closing.trim_end_matches([' ', '\t']);
				let leading = if before.is_empty() {
					String::new()
				} else {
					format!("{before} ")
				};

				closing.clear();
				(leading, String::new(), false, indentation.to_owned())
			}
			None => {
				let line = format!("{indentation}\t");

				// A comment in the empty container stays, before the new entry. The entry's own line takes `line`, the indentation its value is written for, whatever the closing bracket's line had.
				let leading = if tree::has_line_feed(closing)
					&& own_line(closing).trim_matches([' ', '\t']).is_empty()
				{
					format!("{}{line}", comment_lines(closing))
				} else {
					format!("{}\n{line}", closing.trim_end_matches([' ', '\t']))
				};

				*closing = indentation.to_owned();
				(leading, "\n".to_owned(), false, line)
			}
		};

		*entry.node_mut() = new_node(value, &line, is_one_line);
		let separator = entry.separator_mut();
		separator.decor = tree::Decor { leading, trailing };
		separator.has_comma = has_comma;
		entries.insert(index, entry);

		// The entry and the one before it get the comma they need when they share a line with the next one, as the printer writes it, so a later change that puts a line break after them keeps it, as parsing the printed text would.
		for position in index.saturating_sub(1)..=index {
			if tree::shares_line_with_next(entries, position) {
				entries[position].separator_mut().has_comma = true;
			}
		}
	}
}

/**
Removes what is at `path` inside `node`.
*/
fn remove_in(
	node: &mut Node,
	path: &[PathSegment<'_>],
	full_path: &[PathSegment<'_>],
) -> Result<bool, Error> {
	// Where `path` starts in `full_path`.
	let offset = full_path.len() - path.len();

	// The path is never empty here: `path_segments` checks it, and a recursive call gets at least one segment.
	match (node, path[0]) {
		(Node::Object(object), PathSegment::Key(key)) => {
			// Removing a member that is not there changes nothing, and so does removing something below it.
			let Some(index) = object.position(key) else {
				return Ok(false);
			};

			if path.len() > 1 {
				return remove_in(
					object.members_mut()[index].value_mut(),
					&path[1..],
					full_path,
				);
			}

			Ok(object
				.entries_mut()
				.remove_where(|position, _| position == index))
		}
		(Node::Array(array), PathSegment::Index(index)) => {
			// Removing an item that is not there changes nothing, at the end of the array or past it, and so does removing something below it.
			if index >= array.items().len() {
				return Ok(false);
			}

			if path.len() > 1 {
				return remove_in(array.items_mut()[index].value_mut(), &path[1..], full_path);
			}

			Ok(array
				.entries_mut()
				.remove_where(|position, _| position == index))
		}
		(node, _) => Err(segment_error(node, full_path, offset)),
	}
}

impl<T: Entry> Entries<'_, T> {
	/**
	Removes the entries that `is_removed` picks, in one pass, and returns whether there were any. A removed entry takes the comments it owns, which are its decor on its own line and, for the last entry, the closing decor on its line. The comment lines above a removed entry stay, and move to the start of the next entry's leading decor, or to the container's closing decor when no entry follows. When the removed entry was the first, the next one takes its place on its line. A removed entry takes its comma. When the removed entries are the last ones and the last of them has no comma, the comma directly before them goes too, when only spaces, tabs, and the comments the first of them owns are between, so `[1, 2]` and `[1, /* note */ 2]` become `[1]`, as in the JavaScript reference.
	*/
	fn remove_where(mut self, mut is_removed: impl FnMut(usize, &T) -> bool) -> bool {
		self.end_last_line();
		let Self {
			list: entries,
			closing,
			is_braced,
		} = self;
		let count = entries.len();
		let mut kept: Vec<T> = Vec::with_capacity(count);
		// The comment lines of the entries removed since the last kept one, collected once, so removing many entries takes linear time.
		let mut moved = String::new();
		// Where in `moved` each removed entry was, after its comment lines.
		let mut removed_at = Vec::new();
		// The text before the first of the removed entries on its line, such as its indentation, which what follows them on that line takes, as it takes their place.
		let mut indentation = None;
		// Whether the next entry starts a line, whatever its own leading decor is.
		let mut starts_line = !is_braced;
		// Whether the last entry was removed with the line feed that ended the line of the token before it.
		let mut ended_line = false;
		// Whether the last entry was removed without a comma after it.
		let mut is_last_removed_without_comma = false;
		// Whether only spaces, tabs, and the comments it owns are between the first entry removed after the last kept one and the comma of that kept one.
		let mut is_comma_before_removed_spaced = false;
		// Whether the closing decor is on the line of the last entry, which was removed, so it holds only comments that entry owns.
		let mut is_closing_owned = false;

		for (index, mut entry) in std::mem::take(entries).into_iter().enumerate() {
			let decor = &entry.separator().decor;

			// What comes after a removed entry that ended the line of the token before it stays on a line of its own.
			if ended_line {
				end_line(&mut kept, &mut moved);
				indentation = None;
				starts_line = true;
			}

			if is_removed(index, &entry) {
				ended_line = !starts_line
					&& !tree::has_line_feed(&decor.leading)
					&& tree::has_line_feed(&decor.trailing);

				if indentation.is_none() {
					indentation = Some(tree::indentation_of(own_line(&decor.leading)).to_owned());
				}

				// What follows an entry removed with the end of its line starts a line of its own.
				if tree::has_line_feed(&decor.trailing) {
					indentation = None;
				}

				// The comments before the entry on its line are its own, so only they and spaces and tabs may be between it and the comma before it.
				if removed_at.is_empty() {
					is_comma_before_removed_spaced = kept
						.last()
						.is_some_and(|last| last.separator().decor.trailing.is_empty())
						&& !tree::has_line_feed(&decor.leading);
				}

				starts_line = starts_line || tree::has_line_feed(&decor.leading);
				moved.push_str(comment_lines(&decor.leading));
				removed_at.push(moved.len());

				is_last_removed_without_comma = index + 1 == count && !entry.separator().has_comma;
				is_closing_owned =
					is_last_removed_without_comma && !tree::has_line_feed(&decor.trailing);

				continue;
			}

			ended_line = false;

			if !removed_at.is_empty() || indentation.is_some() {
				let leading = &entry.separator().decor.leading;
				let own = own_line(leading);

				// The next item takes the removed one's place on its line, with its own comments before it on that line.
				let own = match indentation.take() {
					Some(indentation) => {
						format!("{indentation}{}", own.trim_start_matches([' ', '\t']))
					}
					None => own.to_owned(),
				};

				let leading = format!("{moved}{}{own}", comment_lines(leading));
				entry.separator_mut().decor.leading = without_extra_blank_lines(
					&leading,
					&removed_at,
					Surroundings::before_entry(kept.last(), is_braced),
				);
				moved.clear();
				removed_at.clear();
			}

			let separator = entry.separator();
			starts_line = !is_braced || tree::has_line_feed(&separator.decor.trailing);
			kept.push(entry);
		}

		if ended_line {
			end_line(&mut kept, &mut moved);
		}

		// The comments after the last entry on its line, before the closing bracket, are its own, so they go with it.
		if is_closing_owned {
			closing.clear();
		}

		// A comment before the closing bracket on the line of the removed entries, after the comma of the last of them, takes their place on that line. It belongs to no entry, so it stays.
		if let Some(indentation) = indentation
			&& !tree::has_line_feed(closing)
			&& !closing.trim_matches([' ', '\t']).is_empty()
		{
			*closing = format!("{indentation}{}", closing.trim_start_matches([' ', '\t']));
		}

		if !moved.is_empty() {
			moved.push_str(closing);
			*closing = without_extra_blank_lines(
				&moved,
				&removed_at,
				Surroundings::before_closing(kept.last(), is_braced),
			);
		}

		*entries = kept;

		if is_last_removed_without_comma
			&& is_comma_before_removed_spaced
			&& let Some(last) = entries.last_mut()
		{
			let separator = last.separator_mut();
			separator.has_comma = false;

			// Without its comma, the text between the value and where the comma was is after the value, so it is split as the parser splits the text after a value: up to its first line feed for the entry, and the rest for the container. The spaces and tabs directly before the comma go with it, as in the JavaScript reference.
			if !separator.before_comma.is_empty() {
				let before_comma = std::mem::take(&mut separator.before_comma);
				let text = before_comma.trim_end_matches([' ', '\t']).to_owned()
					+ &separator.decor.trailing;
				// Without a line feed, all of it is before the closing bracket, on the entry's line, which the parser gives to the container.
				let line_end = tree::first_line_end(&text).unwrap_or(0);
				separator.decor.trailing = text[..line_end].to_owned();
				closing.insert_str(0, &text[line_end..]);
			}
		}

		let is_removed = entries.len() < count;

		// A container that the removal emptied closes up to `[]` or `{}`. One that was empty already keeps its layout, because nothing changed.
		if is_removed && entries.is_empty() && closing.trim_matches([' ', '\t', '\n']).is_empty() {
			closing.clear();
		}

		is_removed
	}
}

/**
Where decor that a removal joined is in its container or document.
*/
#[derive(Clone, Copy)]
struct Surroundings {
	/**
	Whether the decor starts a line. It does after an entry whose line it ended, and at the start of a document. The decor after an opening bracket starts on the bracket's line.
	*/
	starts_line: bool,
	/**
	Whether nothing comes before it.
	*/
	at_start: bool,
	/**
	Whether nothing comes after it.
	*/
	at_end: bool,
}

impl Surroundings {
	/**
	The decor before an entry, which comes after `previous`, or first when there is none.
	*/
	fn before_entry<T: Entry>(previous: Option<&T>, is_braced: bool) -> Self {
		Self {
			starts_line: previous.map_or(!is_braced, |previous| {
				tree::has_line_feed(&previous.separator().decor.trailing)
			}),
			at_start: previous.is_none(),
			at_end: false,
		}
	}

	/**
	The decor before a closing bracket or the end of a document, which comes after `previous`, or first when there is none.
	*/
	fn before_closing<T: Entry>(previous: Option<&T>, is_braced: bool) -> Self {
		Self {
			at_end: true,
			..Self::before_entry(previous, is_braced)
		}
	}
}

/**
Decor that a removal joined, as in the JavaScript reference: the blank lines between entries removed together go with them, and the removal takes one more blank line that it would leave next to another one, or directly after an opening bracket or the start of a document, or directly before a closing bracket or the end of a document. Only a run of blank lines at a place in `removed_at`, where an entry was removed, changes, so every other blank line stays as written. A line with a comment is not blank.
*/
fn without_extra_blank_lines(
	decor: &str,
	removed_at: &[usize],
	surroundings: Surroundings,
) -> String {
	let Surroundings {
		starts_line,
		at_start,
		at_end,
	} = surroundings;
	let mut output = String::new();
	// The run of blank lines since the last line that is not blank, as a range of `decor`.
	let mut blank_run: Option<std::ops::Range<usize>> = None;
	let mut has_content = !at_start;
	let mut line_start = 0;

	let end_run =
		|output: &mut String, run: std::ops::Range<usize>, is_at_start: bool, is_at_end: bool| {
			// The offsets are in order, so a binary search keeps a removal of many entries linear.
			// An entry removed at `run.start` or at `run.end` counts as in the run.
			let first = removed_at.partition_point(|&offset| offset < run.start);
			let last = removed_at.partition_point(|&offset| offset <= run.end);

			let (Some(&first_removed), Some(&last_removed)) = (
				removed_at[first..last].first(),
				removed_at[first..last].last(),
			) else {
				output.push_str(&decor[run]);
				return;
			};

			let mut before = run.start..first_removed;
			let mut after = last_removed..run.end;

			// Both ranges start and end at line starts, so each branch removes one whole blank line: the last one before the removed entries, or else the first one after them.
			if !before.is_empty() && (!after.is_empty() || is_at_end) {
				before.end = decor[before.start..before.end - 1]
					.rfind('\n')
					.map_or(before.start, |index| before.start + index + 1);
			} else if !after.is_empty() && is_at_start {
				after.start = decor[after.clone()]
					.find('\n')
					.map_or(after.end, |index| after.start + index + 1);
			}

			output.push_str(&decor[before]);
			output.push_str(&decor[after]);
		};

	for (part, range) in tree::decor_parts(decor) {
		if part != tree::DecorPart::LineFeed {
			continue;
		}

		let line = line_start..range.end;
		let is_empty = decor[line.clone()]
			.bytes()
			.all(|byte| matches!(byte, b' ' | b'\t' | b'\n'));
		// When the decor does not start a line, its first line feed ends the line before it, such as the line of an opening bracket.
		let ends_earlier_line = line_start == 0 && !starts_line;
		line_start = range.end;

		if is_empty && !ends_earlier_line {
			blank_run = Some(blank_run.map_or(line.clone(), |run| run.start..line.end));
			continue;
		}

		if let Some(run) = blank_run.take() {
			end_run(&mut output, run, !has_content, false);
		}

		// A comment on the line before, such as the line of an opening bracket, is not content inside the container.
		has_content |= !is_empty && !ends_earlier_line;
		output.push_str(&decor[line]);
	}

	let rest = &decor[line_start..];

	if let Some(run) = blank_run {
		let is_at_end = at_end && rest.bytes().all(|byte| byte == b' ' || byte == b'\t');
		end_run(&mut output, run, !has_content, is_at_end);
	}

	output.push_str(rest);
	output
}

impl FromStr for Document {
	type Err = Error;

	/**
	Parses a document, with the same checks and errors as [`from_str`](crate::from_str).
	*/
	fn from_str(text: &str) -> Result<Self, Error> {
		let parts = tree::build(text, crate::parse::parse(text)?);

		Ok(Self {
			outer: parts.outer,
			root: parts.root,
			source: text.to_owned(),
		})
	}
}

/**
Prints the document: the exact text it was parsed from, with any changes.
*/
impl Display for Document {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		let mut printer = Printer::new(false);

		printer.document(&self.outer, &self.root);
		formatter.write_str(&printer.output)
	}
}
