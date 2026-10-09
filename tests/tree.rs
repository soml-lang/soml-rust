/*!
The syntax tree: lossless printing, positions, comments, and changes with `set` and `remove`.
*/

use soml::tree::{CommentKind, Decor, Key, Member, Node, Radix, StringStyle};
use soml::{Document, PathSegment, Value};
use std::fs;
use std::path::Path;

fn document(text: &str) -> Document {
	text.parse()
		.unwrap_or_else(|error| panic!("{error}: {text:?}"))
}

/**
Every valid case of the conformance suite, as text.
*/
fn corpus() -> Vec<(String, String)> {
	fn walk(directory: &Path, cases: &mut Vec<(String, String)>) {
		for entry in fs::read_dir(directory).expect("the corpus directory exists") {
			let path = entry.expect("the corpus directory is readable").path();

			if path.is_dir() {
				walk(&path, cases);
			} else if path.to_str().is_some_and(|path| {
				// The canonical and formatted forms are companions of a case, not cases.
				path.ends_with(".soml")
					&& !path.ends_with(".canonical.soml")
					&& !path.ends_with(".formatted.soml")
			}) {
				let bytes = fs::read(&path).expect("the case is readable");

				// A case that is not UTF-8 is only valid for `from_slice` tests, which do not apply here.
				if let Ok(text) = String::from_utf8(bytes) {
					cases.push((path.display().to_string(), text));
				}
			}
		}
	}

	let mut cases = Vec::new();
	walk(
		&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/conformance/valid"),
		&mut cases,
	);
	cases.sort();
	cases
}

#[test]
fn every_valid_corpus_case_prints_back_unchanged() {
	let mut failures = Vec::new();

	for (name, text) in corpus() {
		match text.parse::<Document>() {
			Ok(document) if document.to_string() == text => {}
			Ok(document) => failures.push(format!(
				"{name}: printed {:?}, expected {text:?}",
				document.to_string()
			)),
			Err(error) => failures.push(format!("{name}: rejected: {error}")),
		}
	}

	assert!(
		failures.is_empty(),
		"{} failures:\n{}",
		failures.len(),
		failures.join("\n")
	);
}

#[test]
fn every_valid_corpus_case_has_the_same_value_as_from_str() {
	for (name, text) in corpus() {
		let expected: Value = soml::from_str(&text).expect("a valid case");
		assert_eq!(
			document(&text).to_value().expect("a valid case"),
			expected,
			"{name}"
		);
	}
}

#[test]
fn comments_after_a_change_have_the_spans_of_the_printed_text_on_the_corpus() {
	let mut failures = Vec::new();

	for (name, text) in corpus() {
		let document = document(&text);
		let value = document.to_value().expect("a valid case");
		let mut changed = Vec::new();

		// Replace, add next to, and remove each top-level member or item.
		match &value {
			Value::Object(object) => {
				for key in object.keys() {
					for change in [
						&(|document: &mut Document| document.set([key.as_str()], 1).is_ok())
							as &dyn Fn(&mut Document) -> bool,
						&|document| document.set([key.as_str(), "added"], 1).is_ok(),
						&|document| document.remove([key.as_str()]).is_ok(),
					] {
						let mut document = document.clone();

						if change(&mut document) {
							changed.push(document);
						}
					}
				}
			}
			Value::Array(items) => {
				for index in 0..=items.len() {
					let mut set = document.clone();

					if set.set([index], 1).is_ok() {
						changed.push(set);
					}

					let mut removed = document.clone();

					if removed.remove([index]).is_ok() {
						changed.push(removed);
					}
				}
			}
			_ => unreachable!("a document is an object or an array"),
		}

		for document in std::iter::once(&document).chain(&changed) {
			let printed = document.to_string();
			let reparsed = self::document(&printed).comments();

			if document.comments() != reparsed {
				failures.push(format!("{name}: {printed:?}"));
			}
		}
	}

	assert!(
		failures.is_empty(),
		"{} failures:\n{}",
		failures.len(),
		failures.join("\n")
	);
}

#[test]
fn an_invalid_document_is_rejected_with_the_same_error() {
	let text = "a: 1\na: 2";
	let error = text.parse::<Document>().unwrap_err();
	assert_eq!(
		error.to_string(),
		soml::from_str::<Value>(text).unwrap_err().to_string()
	);
}

#[test]
fn a_document_that_ends_in_a_comment_without_a_line_feed_prints_back_unchanged() {
	for text in ["a: 1 # end", "a: 1\n# end", "[1] # end", "{a: 1} /* end */"] {
		assert_eq!(document(text).to_string(), text);
	}
}

#[test]
fn nodes_have_spans_and_values() {
	let text = "name: 'api'\nport: 0x1F90\n'limits.memory': 512\nhosts: [\"a\", '''\n\tb\n\t''']\n";
	let document = document(text);
	let root = document.root().as_object().expect("an object");
	assert!(!root.is_braced());
	assert_eq!(root.members().len(), 4);

	let port = document
		.get(["port"])
		.and_then(Node::as_scalar)
		.expect("a scalar");
	assert_eq!(port.value(), &Value::Int(8080));
	assert_eq!(port.raw(), Some("0x1F90"));
	assert_eq!(port.radix(), Some(Radix::Hexadecimal));
	assert_eq!(Radix::Hexadecimal as u32, 16);
	assert_eq!(&text[port.span().expect("a span")], "0x1F90");

	let member = &root.members()[2];
	assert_eq!(member.key().to_string(), "'limits.memory'");
	assert_eq!(member.key().value(), "limits.memory");
	assert_eq!(member.key().style(), StringStyle::Literal);
	assert_eq!(
		&text[member.key().span().expect("a span")],
		"'limits.memory'"
	);
	assert_eq!(
		&text[member.span().expect("a span")],
		"'limits.memory': 512"
	);
	assert_eq!(root.members()[0].key().style(), StringStyle::Bare);

	let name = document
		.get(["name"])
		.and_then(Node::as_scalar)
		.expect("a scalar");
	assert_eq!(name.string_style(), Some(StringStyle::Literal));

	let first = document
		.get([PathSegment::from("hosts"), 0.into()])
		.and_then(Node::as_scalar)
		.expect("a scalar");
	assert_eq!(first.string_style(), Some(StringStyle::Escaped));

	let second = document
		.get([PathSegment::from("hosts"), 1.into()])
		.and_then(Node::as_scalar)
		.expect("a scalar");
	assert_eq!(second.value(), &Value::String("b".to_owned()));
	assert_eq!(second.string_style(), Some(StringStyle::LiteralBlock));

	assert_eq!(
		document
			.get(["limits.memory"])
			.and_then(Node::as_scalar)
			.map(|scalar| scalar.value().clone()),
		Some(Value::Int(512))
	);
	assert_eq!(
		document.position(port.span().expect("a span").start),
		soml::LineColumn { line: 2, column: 7 }
	);
}

#[test]
fn get_returns_none_for_a_missing_path() {
	let document = document("server: {host: 'a', port: 1}\n'server.host': 2");
	assert!(document.get(["server"]).is_some());
	assert!(document.get(["server", "host"]).is_some());
	assert!(document.get(["server.host"]).is_some());
	assert!(document.get(["server", "host.x"]).is_none());
	assert!(document.get(["missing"]).is_none());
	assert!(document.get(["server", "host", "deeper"]).is_none());
}

#[test]
fn comments_are_found_with_their_spans() {
	let text = "# One\na: 1 # Two\n/* Three */ b: [1, /* Four */ 2]\n";
	let comments = document(text).comments();
	let texts: Vec<&str> = comments
		.iter()
		.map(|comment| comment.text.as_str())
		.collect();
	assert_eq!(texts, [" One", " Two", " Three ", " Four "]);
	assert_eq!(comments[0].kind, CommentKind::Line);
	assert_eq!(comments[2].kind, CommentKind::Block);

	for comment in &comments {
		let delimited = &text[comment.span.clone()];
		assert!(delimited.contains(&comment.text), "{delimited:?}");
	}
}

/**
Applies `change` to `before`, and checks the printed text and that it is a valid document.
*/
fn check(before: &str, change: impl FnOnce(&mut Document), after: &str) {
	let mut document = document(before);
	change(&mut document);
	assert_eq!(document.to_string(), after);
	assert!(
		soml::from_str::<Value>(after).is_ok(),
		"not valid: {after:?}"
	);
}

#[test]
fn set_replaces_a_value_and_keeps_its_comments() {
	check(
		"# Head\nport: 0x1F90 # The default.\nname: 'x'\n",
		|document| document.set(["port"], 8080).expect("set"),
		"# Head\nport: 8080 # The default.\nname: 'x'\n",
	);
	check(
		"{a: 1, b: 2}",
		|document| document.set(["b"], "two").expect("set"),
		"{a: 1, b: 'two'}",
	);
	check(
		"a: [1, 2, 3]",
		|document| {
			document
				.set([PathSegment::from("a"), 1.into()], 9.5)
				.expect("set")
		},
		"a: [1, 9.5, 3]",
	);
}

#[test]
fn set_writes_the_value_that_replaces_a_block_string_on_the_line_of_its_key() {
	check(
		"a:\n\t'''\n\tx\n\t'''\nb: 1\n",
		|document| document.set(["a"], 2).expect("set"),
		"a: 2\nb: 1\n",
	);
	check(
		"x: {\n\ta:\n\t\t'''\n\t\tx\n\t\t'''\n}\n",
		|document| document.set(["x", "a"], [1, 2]).expect("set"),
		"x: {\n\ta: [\n\t\t1\n\t\t2\n\t]\n}\n",
	);
	// With a comment between, the value stays on its own line.
	check(
		"a: # c\n\t'''\n\tx\n\t'''\n",
		|document| document.set(["a"], 2).expect("set"),
		"a: # c\n\t2\n",
	);
}

#[test]
fn set_writes_a_new_collection_on_indented_lines() {
	check(
		"server: {\n\tport: 1\n}\n",
		|document| {
			document
				.set(["server", "port"], Value::from_iter([("a", 1), ("b", 2)]))
				.expect("set")
		},
		"server: {\n\tport: {\n\t\ta: 1\n\t\tb: 2\n\t}\n}\n",
	);
	check(
		"a: 1\n",
		|document| document.set(["list"], [1, 2]).expect("set"),
		"a: 1\nlist: [\n\t1\n\t2\n]\n",
	);
	check(
		"a: 1\n",
		|document| {
			document
				.set(["empty"], Value::Array(Vec::new()))
				.expect("set")
		},
		"a: 1\nempty: []\n",
	);
}

#[test]
fn set_adds_a_top_level_member_on_a_new_line() {
	check(
		"a: 1\n",
		|document| document.set(["b"], 2).expect("set"),
		"a: 1\nb: 2\n",
	);
	check(
		"a: 1",
		|document| document.set(["b"], 2).expect("set"),
		"a: 1\nb: 2",
	);
	check(
		"a: 1 # note\n# end\n",
		|document| document.set(["b"], 2).expect("set"),
		"a: 1 # note\nb: 2\n# end\n",
	);
	check(
		"'the name': 1\n",
		|document| document.set(["a.b"], 2).expect("set"),
		"'the name': 1\n'a.b': 2\n",
	);
}

#[test]
fn set_adds_a_member_to_a_braced_object_like_its_siblings() {
	check(
		"server: {\n\thost: 'a',\n}\n",
		|document| document.set(["server", "port"], 1).expect("set"),
		"server: {\n\thost: 'a',\n\tport: 1\n}\n",
	);
	check(
		"server: {\n\thost: 'a'\n}\n",
		|document| document.set(["server", "port"], 1).expect("set"),
		"server: {\n\thost: 'a'\n\tport: 1\n}\n",
	);
	check(
		"server: {host: 'a'}\n",
		|document| document.set(["server", "port"], 1).expect("set"),
		"server: {host: 'a', port: 1}\n",
	);
	check(
		"server: {}\n",
		|document| document.set(["server", "port"], 1).expect("set"),
		"server: {\n\tport: 1\n}\n",
	);
	check(
		"a: {\n\tb: {},\n}\n",
		|document| document.set(["a", "b", "c"], 1).expect("set"),
		"a: {\n\tb: {\n\t\tc: 1\n\t},\n}\n",
	);
	check(
		"{\n\ta: 1,\n}\n",
		|document| document.set(["b"], 2).expect("set"),
		"{\n\ta: 1,\n\tb: 2\n}\n",
	);
}

#[test]
fn set_appends_to_an_array_like_its_items() {
	check(
		"a: [1, 2]",
		|document| {
			document
				.set([PathSegment::from("a"), 2.into()], 3)
				.expect("set")
		},
		"a: [1, 2, 3]",
	);
	check(
		"a: [\n\t1,\n]\n",
		|document| {
			document
				.set([PathSegment::from("a"), 1.into()], 2)
				.expect("set")
		},
		"a: [\n\t1,\n\t2\n]\n",
	);
	check(
		"a: []\n",
		|document| {
			document
				.set([PathSegment::from("a"), 0.into()], 1)
				.expect("set")
		},
		"a: [\n\t1\n]\n",
	);
	check(
		"[1]",
		|document| document.set([1], 2).expect("set"),
		"[1, 2]",
	);
}

#[test]
fn set_adds_a_key_without_a_span() {
	let mut document = document("'a': 1\n");
	document.set(["b c"], 2).expect("set");
	let members = document.root().as_object().expect("an object").members();
	assert_eq!(members[0].key().raw(), Some("'a'"));
	assert_eq!(members[1].key().span(), None);
	assert_eq!(members[1].key().raw(), None);
	assert_eq!(members[1].key().style(), StringStyle::Literal);
	assert_eq!(document.to_string(), "'a': 1\n'b c': 2\n");
}

#[test]
fn set_creates_a_missing_parent_with_braces() {
	check(
		"a: 1\n",
		|document| {
			document
				.set(["server", "tls", "enabled"], true)
				.expect("set")
		},
		"a: 1\nserver: {\n\ttls: {\n\t\tenabled: true\n\t}\n}\n",
	);
}

#[test]
fn set_rejects_a_carriage_return_in_any_key_of_the_path_the_same_way() {
	// A key the change adds and a key it nests in a new object both give the error of a key that cannot be written.
	for (text, path) in [
		("a: 1\n", vec!["x\ry"]),
		("a: 1\n", vec!["n", "x\ry"]),
		("a: {}\n", vec!["a", "x\ry"]),
		("a: {}\n", vec!["a", "n", "x\ry"]),
		("[{}]", vec!["x\ry"]),
	] {
		let mut document = document(text);
		let error = if document.root().as_array().is_some() {
			document
				.set([PathSegment::Index(0), PathSegment::Key(path[0])], 1)
				.unwrap_err()
		} else {
			document.set(path.clone(), 1).unwrap_err()
		};
		assert_eq!(error.kind(), soml::ErrorKind::Write, "{path:?}");
		assert_eq!(
			error.message(),
			"A key cannot contain a carriage return (U+000D), because SOML cannot represent one",
			"{path:?}"
		);
		assert_eq!(error.position(), None);
		assert_eq!(document.to_string(), text);
	}
}

#[test]
fn set_rejects_paths_that_do_not_fit() {
	let mut document = document("a: 1\nb: [1]\nserver: {port: 1}\n");
	assert_eq!(
		document.set(["a", "b"], 1).unwrap_err().kind(),
		soml::ErrorKind::Data,
		"through a scalar"
	);
	assert!(
		document
			.set([PathSegment::from("b"), PathSegment::from("x")], 1)
			.is_err(),
		"a key on an array"
	);
	assert!(
		document.set([PathSegment::from("b"), 5.into()], 1).is_err(),
		"past the end of an array"
	);
	assert!(
		document.set([PathSegment::from("a"), 0.into()], 1).is_err(),
		"an index on a scalar"
	);
	assert!(
		document.set([PathSegment::Index(0)], 1).is_err(),
		"an index on an object"
	);
	assert!(
		document.set(Vec::<&str>::new(), 1).is_err(),
		"an empty path"
	);
	assert!(document.set(["c"], f64::NAN).is_err(), "NaN");
	assert!(document.set(["c"], "a\rb").is_err(), "a carriage return");
	assert_eq!(document.to_string(), "a: 1\nb: [1]\nserver: {port: 1}\n");
}

#[test]
fn set_below_an_index_equal_to_the_length_of_an_array_adds_an_item() {
	check(
		"b: []\n",
		|document| {
			document
				.set([PathSegment::from("b"), 0.into(), "c".into()], 1)
				.expect("set");
		},
		"b: [\n\t{\n\t\tc: 1\n\t}\n]\n",
	);
	check(
		"b: [1, 2]\n",
		|document| {
			document
				.set(
					[PathSegment::from("b"), 2.into(), "c".into(), "d".into()],
					1,
				)
				.expect("set");
		},
		"b: [1, 2, {c: {d: 1}}]\n",
	);
}

#[test]
fn set_with_an_index_on_a_missing_array_says_the_array_does_not_exist() {
	let mut document = document("server: {port: 1}\n");

	for (path, message) in [
		(
			vec![PathSegment::from("x"), 0.into()],
			"Cannot edit x[0], because x does not exist, so it has no index 0",
		),
		(
			vec![
				PathSegment::from("server"),
				PathSegment::from("hosts"),
				0.into(),
			],
			"Cannot edit server.hosts[0], because server.hosts does not exist, so it has no index 0",
		),
	] {
		assert_eq!(document.set(path, 1).unwrap_err().message(), message);
	}
}

#[test]
fn remove_takes_the_member_and_the_comment_at_the_end_of_its_line() {
	check(
		"a: 1\nb: 2 # About b.\nc: 3\n",
		|document| assert!(document.remove(["b"]).expect("remove")),
		"a: 1\nc: 3\n",
	);
	check(
		"{a: 1, b: 2}",
		|document| assert!(document.remove(["a"]).expect("remove")),
		"{b: 2}",
	);
	check(
		"{a: 1, b: 2}",
		|document| assert!(document.remove(["b"]).expect("remove")),
		"{a: 1}",
	);
	check(
		"{\n\ta: 1,\n\tb: 2,\n}\n",
		|document| assert!(document.remove(["b"]).expect("remove")),
		"{\n\ta: 1,\n}\n",
	);
	check(
		"{\n\ta: 1,\n\tb: 2,\n}\n",
		|document| assert!(document.remove(["a"]).expect("remove")),
		"{\n\tb: 2,\n}\n",
	);
	check(
		"x: {\n\ta: 1,\n}\n",
		|document| assert!(document.remove(["x", "a"]).expect("remove")),
		"x: {}\n",
	);
	check(
		"a: [1, 2, 3]",
		|document| {
			assert!(
				document
					.remove([PathSegment::from("a"), 1.into()])
					.expect("remove")
			)
		},
		"a: [1, 3]",
	);
}

#[test]
fn remove_keeps_the_comment_lines_above() {
	check(
		"# Head\na: 1\nb: 2\n",
		|document| assert!(document.remove(["a"]).expect("remove")),
		"# Head\nb: 2\n",
	);
	check(
		"a: 1\n\n# Section\nb: 2\nc: 3\n",
		|document| assert!(document.remove(["b"]).expect("remove")),
		"a: 1\n\n# Section\nc: 3\n",
	);
	check(
		"{\n\ta: 1,\n\t# Last\n\tb: 2,\n}\n",
		|document| assert!(document.remove(["b"]).expect("remove")),
		"{\n\ta: 1,\n\t# Last\n}\n",
	);
}

#[test]
fn removing_the_last_top_level_member_leaves_an_empty_object() {
	check(
		"a: 1\n",
		|document| assert!(document.remove(["a"]).expect("remove")),
		"{}\n",
	);
	check(
		"# head\na: 1 # c\n# tail\n",
		|document| assert!(document.remove(["a"]).expect("remove")),
		"# head\n{} # c\n# tail\n",
	);
}

#[test]
fn remove_keeps_the_comment_lines_after_it_on_their_own_lines() {
	check(
		"x: {a: 1,\n# c\n}\n",
		|document| assert!(document.remove(["x", "a"]).expect("remove")),
		"x: {\n# c\n}\n",
	);
	check(
		"x: [1\n# c\n]\n",
		|document| {
			assert!(
				document
					.remove([PathSegment::from("x"), 0.into()])
					.expect("remove")
			)
		},
		"x: [\n# c\n]\n",
	);
	check(
		"x: [1, 2,\n# c\n3]\n",
		|document| {
			assert!(
				document
					.remove([PathSegment::from("x"), 1.into()])
					.expect("remove")
			)
		},
		"x: [1,\n# c\n3]\n",
	);
	// A block comment that starts its line, with a token after it on that line.
	check(
		"{x: 0, a: 1\n/* b */ }",
		|document| assert!(document.remove(["a"]).expect("remove")),
		"{x: 0\n/* b */ }",
	);
	check(
		"{x: 0, a: 1,\n/* b */ c: 2}",
		|document| assert!(document.remove(["a"]).expect("remove")),
		"{x: 0,\n/* b */ c: 2}",
	);
}

#[test]
fn a_member_set_after_removing_every_member_goes_inside_the_braces_left() {
	check(
		"# x\na: 1\n",
		|document| {
			assert!(document.remove(["a"]).expect("remove"));
			document.set(["b"], 2).expect("set");
		},
		"# x\n{\n\tb: 2\n}\n",
	);
	check(
		"# x\na: {b: 1} # c\n# m\n\n# y\n",
		|document| {
			assert!(document.remove(["a"]).expect("remove"));
			assert_eq!(document.to_string(), "# x\n{} # c\n# m\n\n# y\n");
			document.set(["b"], 2).expect("set");
		},
		"# x\n{\n\tb: 2\n} # c\n# m\n\n# y\n",
	);
	check(
		"{a: 1}\n",
		|document| {
			assert!(document.remove(["a"]).expect("remove"));
			document.set(["b"], 2).expect("set");
		},
		"{\n\tb: 2\n}\n",
	);
}

#[test]
fn set_in_an_empty_object_after_a_comment_on_the_closing_line_starts_a_new_line() {
	check(
		"x: {\n/* c */}\n",
		|document| document.set(["x", "b"], 1).expect("set"),
		"x: {\n/* c */\n\tb: 1\n}\n",
	);
}

#[test]
fn a_default_object_is_braced() {
	let mut document = document("a: 1\nc: 2\n");
	*document.get_mut(["a"]).expect("a node") = Node::Object(soml::tree::Object::default());
	assert_eq!(document.to_string(), "a: {}\nc: 2\n");
	assert!(document.to_value().is_ok());
}

#[test]
fn set_in_an_empty_object_after_a_block_comment_that_spans_lines_starts_a_new_line() {
	check(
		"x: {/* a\nb */}\n",
		|document| document.set(["x", "c"], 3).expect("set"),
		"x: {/* a\nb */\n\tc: 3\n}\n",
	);
}

#[test]
fn remove_reports_what_it_did() {
	let mut document = document("a: 1\nb: [1]\n");
	assert!(!document.remove(["missing"]).expect("remove"));
	assert!(
		!document
			.remove([PathSegment::from("b"), 3.into()])
			.expect("remove")
	);
	assert!(document.remove(["a", "b"]).is_err(), "through a scalar");
	assert!(
		document
			.remove([PathSegment::from("b"), PathSegment::from("x")])
			.is_err(),
		"a key on an array"
	);
}

#[test]
fn removing_what_is_not_there_changes_nothing() {
	// An empty container keeps its layout, as in the JavaScript reference.
	for text in [
		"a: {\n}\n",
		"a: { }\n",
		"c: {  }",
		"a: {\n\t\n}\n",
		"{ }",
		"[\n]",
	] {
		let mut document = document(text);

		if document.root().as_array().is_some() {
			assert!(!document.remove([0]).expect("remove"), "{text:?}");
		} else {
			assert!(
				!document.remove(["a", "missing"]).expect("remove"),
				"{text:?}"
			);
			assert!(!document.remove(["missing"]).expect("remove"), "{text:?}");
		}

		assert_eq!(document.to_string(), text);
	}
}

#[test]
fn removing_a_path_that_leads_nowhere_changes_nothing_and_is_not_an_error() {
	for (text, path) in [
		// A missing member.
		("a: 1\n", vec![PathSegment::from("b")]),
		// A missing object on the way.
		("a: 1\n", vec![PathSegment::from("b"), "c".into()]),
		(
			"a: {b: 1}\n",
			vec![PathSegment::from("a"), "x".into(), "y".into()],
		),
		// An index at or past the end of its array, and anything below it.
		("a: [1, 2]\n", vec![PathSegment::from("a"), 2.into()]),
		("a: [1, 2]\n", vec![PathSegment::from("a"), 5.into()]),
		(
			"a: [1, 2]\n",
			vec![PathSegment::from("a"), 5.into(), "x".into()],
		),
		("[1]", vec![PathSegment::from(1), 0.into(), "x".into()]),
		// An index below a value that does not exist.
		("a: 1\n", vec![PathSegment::from("b"), 0.into()]),
		(
			"a: {b: 1}\n",
			vec![PathSegment::from("a"), "x".into(), 0.into()],
		),
		(
			"a: {}\n",
			vec![PathSegment::from("a"), "x".into(), 0.into(), "y".into()],
		),
	] {
		let mut document = document(text);
		assert!(
			!document.remove(path.clone()).expect("remove"),
			"{text:?} {path:?}"
		);
		assert_eq!(document.to_string(), text, "{path:?}");
	}
}

#[test]
fn removing_the_same_path_twice_is_safe() {
	let mut document = document("a: [1, 2]\nb: 1\n");

	for path in [
		vec![PathSegment::from("b")],
		vec![PathSegment::from("a"), 1.into()],
	] {
		assert!(document.remove(path.clone()).expect("remove"), "{path:?}");
		assert!(!document.remove(path.clone()).expect("remove"), "{path:?}");
	}

	assert_eq!(document.to_string(), "a: [1]\n");
}

#[test]
fn a_path_that_does_not_fit_the_document_is_an_error_for_set_and_remove() {
	// The messages are those of the JavaScript reference.
	for (text, path, message) in [
		(
			"a: {b: 1}\n",
			vec![PathSegment::from("a"), 0.into()],
			"Cannot edit a[0], because a is an object, so it needs a key, not an index",
		),
		(
			"a: [1]\n",
			vec![PathSegment::from("a"), "x".into()],
			"Cannot edit a.x, because a is an array, so it needs an index, not a key",
		),
		(
			"a: [[1]]\n",
			vec![PathSegment::from("a"), 0.into(), "x".into()],
			"Cannot edit a[0].x, because a[0] is an array, so it needs an index, not a key",
		),
		(
			"a: 1\n",
			vec![PathSegment::from("a"), "x".into()],
			"Cannot edit a.x, because a is not an object or an array",
		),
		(
			"a: 1\n",
			vec![PathSegment::from("a"), 0.into()],
			"Cannot edit a[0], because a is not an object or an array",
		),
	] {
		let mut document = document(text);
		assert_eq!(
			document.set(path.clone(), 1).unwrap_err().message(),
			message,
			"set {text:?}"
		);
		assert_eq!(
			document.remove(path).unwrap_err().message(),
			message,
			"remove {text:?}"
		);
		assert_eq!(document.to_string(), text);
	}

	// Only a new value needs the item before it, and an array that exists to hold an index.
	for (text, path, message) in [
		(
			"a: [1]\n",
			vec![PathSegment::from("a"), 3.into()],
			"Cannot edit a[3], because the array at a has 1 item. Add an item at index 1",
		),
		(
			"a: 1\n",
			vec![PathSegment::from("b"), 0.into()],
			"Cannot edit b[0], because b does not exist, so it has no index 0",
		),
	] {
		let mut document = document(text);
		assert_eq!(
			document.set(path.clone(), 1).unwrap_err().message(),
			message,
			"set {text:?}"
		);
		assert!(!document.remove(path).expect("remove"), "remove {text:?}");
		assert_eq!(document.to_string(), text);
	}
}

#[test]
fn node_level_changes_print_and_are_checked_by_to_value() {
	let mut document = document("a: 1\n");
	let root = document.root_mut().as_object_mut().expect("an object");
	root.members_mut()
		.push(Member::new(Key::new("b"), 2).expect("a member"));
	assert_eq!(document.to_string(), "a: 1\nb: 2");
	assert_eq!(
		document.to_value().expect("valid").get("b"),
		Some(&Value::Int(2))
	);

	let root = document.root_mut().as_object_mut().expect("an object");
	root.members_mut()
		.push(Member::new(Key::new("a"), 3).expect("a member"));
	assert_eq!(
		document.to_value().unwrap_err().message(),
		"Duplicate key a"
	);
}

#[test]
fn a_member_added_after_a_line_comment_at_the_end_goes_on_the_next_line() {
	let mut document = document("a: 1 # c");
	let root = document.root_mut().as_object_mut().expect("an object");
	root.members_mut()
		.push(Member::new(Key::new("b"), 2).expect("a member"));
	assert_eq!(document.to_string(), "a: 1 # c\nb: 2");
	assert_eq!(document.format(), "a: 1 # c\nb: 2\n");
}

#[test]
fn set_after_a_last_line_without_a_line_feed_keeps_the_spaces_of_its_line_comment() {
	// The spaces at the end of a line comment are part of it. Other spaces at the end of the line are layout, and they go.
	for (before, after) in [
		("a: 1 # note  ", "a: 1 # note  \nb: 2"),
		("a: 1 # note\t", "a: 1 # note\t\nb: 2"),
		("a: 1 /* note */  ", "a: 1 /* note */\nb: 2"),
		("a: 1  ", "a: 1\nb: 2"),
	] {
		let mut document = document(before);
		let comments = document.comments();
		document.set(["b"], 2).expect("set");
		assert_eq!(document.to_string(), after);
		assert_eq!(
			document.comments()[..comments.len()],
			comments,
			"{before:?}"
		);
	}
}

#[test]
fn a_line_comment_at_the_end_of_changed_decor_keeps_its_line_when_formatted() {
	let mut document = document("a: 1\n");
	let members = document
		.root_mut()
		.as_object_mut()
		.expect("an object")
		.members_mut();
	members[0].decor_mut().trailing = " # c".to_owned();
	let mut member = Member::new(Key::new("b"), 2).expect("a member");
	member.decor_mut().leading = "# d\n".to_owned();
	members.push(member);
	assert_eq!(document.to_string(), "a: 1 # c\n# d\nb: 2");
	assert_eq!(document.format(), "a: 1 # c\n# d\nb: 2\n");
}

#[test]
fn a_line_comment_at_the_end_of_changed_decor_is_ended_before_the_end_of_the_document() {
	let mut document = document("a: 1");
	let root = document.root_mut().as_object_mut().expect("an object");
	*root.closing_decor_mut() = "/* e */".to_owned();
	root.members_mut()[0].decor_mut().trailing = " # c".to_owned();
	assert_eq!(document.to_string(), "a: 1 # c\n/* e */");
	assert_eq!(document.format(), "a: 1 # c\n/* e */\n");
}

#[test]
fn changed_decor_that_ends_in_a_line_comment_is_ended_before_what_follows() {
	let mut document = document("{a: 1, b: 2}");
	let root = document.root_mut().as_object_mut().expect("an object");
	root.members_mut()[0].decor_mut().trailing = " # note".to_owned();
	assert_eq!(document.to_string(), "{a: 1, # note\n b: 2}");
	assert!(document.to_value().is_ok());
}

#[test]
fn remove_keeps_a_block_comment_that_spans_lines_whole() {
	check(
		"{\n\ta: 1, # x\n\t/* t\nwo */ b: 2,\n}\n",
		|document| assert!(document.remove(["b"]).expect("remove")),
		"{\n\ta: 1, # x\n}\n",
	);
	check(
		"{\n\ta: 1,\n\t/* t\nwo */\n\tb: 2,\n\tc: 3,\n}\n",
		|document| assert!(document.remove(["b"]).expect("remove")),
		"{\n\ta: 1,\n\t/* t\nwo */\n\tc: 3,\n}\n",
	);
}

#[test]
fn a_top_level_object_without_braces_spans_its_members() {
	let text = "# Head\na: 1\nb: 2 # Tail\n";
	assert_eq!(
		&text[document(text).root().span().expect("a span")],
		"a: 1\nb: 2"
	);
}

#[test]
fn remove_takes_the_comma_before_the_last_entries_only_when_spaces_are_between() {
	let remove_item = |document: &mut Document| {
		assert!(
			document
				.remove([PathSegment::from("a"), 1.into()])
				.expect("remove")
		);
	};

	// The other entries keep their commas, as in the JavaScript reference.
	check("a: [\n\t1,\n\t2\n]", remove_item, "a: [\n\t1,\n]");
	check("a: [1,\n2]", remove_item, "a: [1,\n]");
	check("a: [\n\t1\n\t2,\n]", remove_item, "a: [\n\t1\n]");
	check(
		"{\n\ta: 1\n\tb: 2,\n}\n",
		|document| assert!(document.remove(["b"]).expect("remove")),
		"{\n\ta: 1\n}\n",
	);
	check("a: [1, 2, 3,]", remove_item, "a: [1, 3,]");
	check(
		"[1, 2, 3,]",
		|document| assert!(document.remove([2]).expect("remove")),
		"[1, 2,]",
	);
	// With only spaces between, the comma before the last entry goes too, so no trailing comma is left.
	check("a: [1, 2]", remove_item, "a: [1]");
	// The spaces and tabs before that comma go with it.
	check("a: [1 , 2]", remove_item, "a: [1]");
}

#[test]
fn remove_keeps_a_comment_before_the_comma_it_drops() {
	check(
		"{a: 1 /* c */, b: 2}",
		|document| assert!(document.remove(["b"]).expect("remove")),
		"{a: 1 /* c */}",
	);
	check(
		"[1 /* x */, 2]",
		|document| assert!(document.remove([1]).expect("remove")),
		"[1 /* x */]",
	);
}

#[test]
fn remove_of_the_first_item_keeps_a_comment_in_front_of_the_next() {
	check(
		"a: 1\n/* c */ b: 2",
		|document| assert!(document.remove(["a"]).expect("remove")),
		"/* c */ b: 2",
	);
	check(
		"{a: 1,\n\t/* c */ b: 2}",
		|document| assert!(document.remove(["a"]).expect("remove")),
		"{\n\t/* c */ b: 2}",
	);
	check(
		"[1, /* c */ 2]",
		|document| assert!(document.remove([0]).expect("remove")),
		"[/* c */ 2]",
	);
}

#[test]
fn set_keeps_the_indentation_of_an_indented_first_line() {
	check(
		"\ta: 1\n",
		|document| document.set(["b"], 2).expect("set"),
		"\ta: 1\n\tb: 2\n",
	);
	check(
		"  a: {\n  }",
		|document| document.set(["a", "b"], 2).expect("set"),
		"  a: {\n  \tb: 2\n  }",
	);
}

#[test]
fn set_refuses_a_change_that_makes_the_document_invalid_and_keeps_it_as_it_was() {
	// The top-level object and 98 more are 99 levels, so the leaf can become one more array but not two.
	let text = format!("{}a: 1{}", "a: {".repeat(98), "}".repeat(98));
	let mut deep = document(&text);
	let path = vec!["a"; 99];
	let error = deep
		.set(path.clone(), Value::Array(vec![Value::Array(Vec::new())]))
		.unwrap_err();
	// The place the error is about is in the rejected text, not in the document, so the error has no position.
	assert_eq!(
		(error.kind(), error.message(), error.position()),
		(
			soml::ErrorKind::Write,
			"The document is nested more than 100 levels deep",
			None
		)
	);
	assert_eq!(deep.to_string(), text);
	assert!(deep.set(path, Value::Array(Vec::new())).is_ok());

	let mut document = document("a: 1");
	assert_eq!(
		document.set(["x\ry"], 1).unwrap_err().kind(),
		soml::ErrorKind::Write
	);
	assert_eq!(
		document.set(["b"], f64::NAN).unwrap_err().kind(),
		soml::ErrorKind::Write
	);
	assert_eq!(document.to_string(), "a: 1");
	assert!(Member::new(Key::new("a\rb"), 1).is_err());
}

#[test]
fn a_comment_at_the_end_of_a_file_without_a_line_feed_stays_on_its_line() {
	check(
		"a: 1 # c",
		|document| document.set(["b"], 2).expect("set"),
		"a: 1 # c\nb: 2",
	);
	check(
		"a: 1\nb: 2 # t",
		|document| assert!(document.remove(["b"]).expect("remove")),
		"a: 1\n",
	);
}

#[test]
fn comments_does_not_panic_on_an_unterminated_block_comment_in_changed_decor() {
	let mut document = document("a: 1");
	document
		.root_mut()
		.as_object_mut()
		.expect("an object")
		.members_mut()[0]
		.decor_mut()
		.leading = "/* éa".to_owned();
	assert_eq!(document.comments()[0].text, " éa");
}

#[test]
fn a_value_can_be_replaced_through_get_mut_and_node_new() {
	let mut document = document("server: {port: 1} # The default.\n");
	*document.get_mut(["server", "port"]).expect("a node") = Node::new(8080).expect("a node");
	assert_eq!(
		document.to_string(),
		"server: {port: 8080} # The default.\n"
	);
	assert!(Node::new(f64::NAN).is_err());
}

#[test]
fn set_into_an_empty_container_keeps_its_comment() {
	check(
		"a: { /* c */\n}",
		|document| document.set(["a", "b"], 1).expect("set"),
		"a: { /* c */\n\tb: 1\n}",
	);
	check(
		"a: [ /* c */\n]",
		|document| {
			document
				.set([PathSegment::from("a"), 0.into()], 1)
				.expect("set")
		},
		"a: [ /* c */\n\t1\n]",
	);
	check(
		"{ }",
		|document| document.set(["b"], 1).expect("set"),
		"{\n\tb: 1\n}",
	);

	// A comment between brackets on one line keeps the container on one line, so the new entry goes after the comment, before the closing bracket.
	check(
		"a: { /* c */ }",
		|document| document.set(["a", "b"], 1).expect("set"),
		"a: { /* c */ b: 1}",
	);
	check(
		"a: [ /* c */ ]",
		|document| {
			document
				.set([PathSegment::from("a"), 0.into()], 1)
				.expect("set")
		},
		"a: [ /* c */ 1]",
	);
	check(
		"[/* None yet */]",
		|document| document.set([0], 1).expect("set"),
		"[/* None yet */ 1]",
	);
}

#[test]
fn set_writes_a_new_value_in_a_container_on_one_line_on_one_line() {
	// The expected text is what the JavaScript reference writes.
	let cases: [(&str, Vec<PathSegment<'static>>, Value, &str); 7] = [
		(
			"a: [1, 2]",
			vec!["a".into(), 2.into()],
			soml::soml!({"b": 1, "c": [true]}),
			"a: [1, 2, {b: 1, c: [true]}]",
		),
		(
			"a: {x: 1}",
			vec!["a".into(), "x".into()],
			soml::soml!([1, {"y": 2}]),
			"a: {x: [1, {y: 2}]}",
		),
		(
			"a: {x: 1}",
			vec!["a".into(), "y".into(), "z".into()],
			soml::soml!(2),
			"a: {x: 1, y: {z: 2}}",
		),
		(
			"a: {p: {b: 1}}",
			vec!["a".into(), "p".into(), "c".into(), "d".into()],
			soml::soml!(2),
			"a: {p: {b: 1, c: {d: 2}}}",
		),
		(
			"a: [{x: 1}]",
			vec!["a".into(), 0.into()],
			soml::soml!({"y": [1]}),
			"a: [{y: [1]}]",
		),
		(
			"a: {\n\tx: [1, 2]\n}",
			vec!["a".into(), "x".into(), 2.into()],
			soml::soml!({"b": 1}),
			"a: {\n\tx: [1, 2, {b: 1}]\n}",
		),
		// A top-level object without braces is never on one line.
		(
			"a: 1",
			vec!["a".into()],
			soml::soml!({"b": 1}),
			"a: {\n\tb: 1\n}",
		),
	];

	for (before, path, value, after) in cases {
		check(
			before,
			|document| document.set(path, value).expect("set"),
			after,
		);
		assert_eq!(soml::format(after).expect("valid"), format!("{after}\n"));
	}
}

#[test]
fn set_into_an_empty_container_inside_a_container_on_one_line_stays_on_that_line() {
	// The expected text is what the JavaScript reference writes.
	let cases: [(&str, Vec<PathSegment<'static>>, Value, &str); 7] = [
		(
			"a: [1, []]",
			vec!["a".into(), 1.into(), 0.into()],
			soml::soml!(2),
			"a: [1, [2]]",
		),
		(
			"a: [1, [ ]]",
			vec!["a".into(), 1.into(), 0.into()],
			soml::soml!(2),
			"a: [1, [2]]",
		),
		(
			"a: {b: 1, c: {}}",
			vec!["a".into(), "c".into(), "d".into()],
			soml::soml!([1]),
			"a: {b: 1, c: {d: [1]}}",
		),
		(
			"a: [1, {/* c */}]",
			vec!["a".into(), 1.into(), "k".into()],
			soml::soml!(2),
			"a: [1, {/* c */ k: 2}]",
		),
		// An empty `[]` or `{}` on its own says nothing about layout, so the new entry goes on a line of its own.
		(
			"deps: {}",
			vec!["deps".into(), "a".into()],
			soml::soml!("1.0"),
			"deps: {\n\ta: '1.0'\n}",
		),
		(
			"a: [ ]",
			vec!["a".into(), 0.into()],
			soml::soml!({"b": [1]}),
			"a: [\n\t{\n\t\tb: [\n\t\t\t1\n\t\t]\n\t}\n]",
		),
		(
			"a: {\n\tx: {}\n}",
			vec!["a".into(), "x".into(), "y".into()],
			soml::soml!({"b": 1}),
			"a: {\n\tx: {\n\t\ty: {\n\t\t\tb: 1\n\t\t}\n\t}\n}",
		),
	];

	for (before, path, value, after) in cases {
		check(
			before,
			|document| document.set(path, value).expect("set"),
			after,
		);
	}
}

#[test]
fn set_indents_by_the_line_an_entry_is_on_when_a_value_before_it_spans_lines() {
	// The entry starts on the last line of the value before it, so that line's indentation is the entry's. The expected text is what the JavaScript reference writes.
	check(
		"{ b: {\n\t\tx: 1\n\t}, c: 1\n}",
		|document| document.set(["d", "e"], 2).expect("set"),
		"{ b: {\n\t\tx: 1\n\t}, c: 1\n\td: {\n\t\te: 2\n\t}\n}",
	);
	check(
		"a: [{\n\t\tx: 1\n\t}, [1]]\n",
		|document| {
			document
				.set([PathSegment::from("a"), 1.into()], vec![1, 2])
				.expect("set")
		},
		"a: [{\n\t\tx: 1\n\t}, [\n\t\t1\n\t\t2\n\t]]\n",
	);
	check(
		"a: [{\n\t\tx: 1\n\t}, 2\n]\n",
		|document| {
			document
				.set([PathSegment::from("a"), 2.into()], vec![3])
				.expect("set")
		},
		"a: [{\n\t\tx: 1\n\t}, 2\n\t[\n\t\t3\n\t]\n]\n",
	);
	check(
		"{a: '''\n\t\tx\n\t\t''', b: 1\n}",
		|document| document.set(["c", "d"], 1).expect("set"),
		"{a: '''\n\t\tx\n\t\t''', b: 1\n\t\tc: {\n\t\t\td: 1\n\t\t}\n}",
	);
	check(
		"{a: # c\n\t1, b: 1\n}",
		|document| document.set(["c"], vec![1]).expect("set"),
		"{a: # c\n\t1, b: 1\n\tc: [\n\t\t1\n\t]\n}",
	);

	// A line feed in a block comment does not start a line of its own.
	check(
		"{\n\ta: /* x\n y */ 1, b: 2\n}",
		|document| document.set(["c"], vec![1]).expect("set"),
		"{\n\ta: /* x\n y */ 1, b: 2\n\tc: [\n\t\t1\n\t]\n}",
	);
	check(
		"{\n\ta: [1 /* x\n y */]}",
		|document| document.set(["c"], vec![1]).expect("set"),
		"{\n\ta: [1 /* x\n y */], c: [\n\t\t1\n\t]}",
	);

	// An entry added after a comma is on the line where the entry before it ends.
	check(
		"{a: [\n\t\t1] }",
		|document| document.set(["n", "m"], 1).expect("set"),
		"{a: [\n\t\t1], n: {\n\t\t\tm: 1\n\t\t} }",
	);
	check(
		"x: {\n\tb: [\n\t\ttrue ],}\n",
		|document| document.set(["x", "z"], vec![1]).expect("set"),
		"x: {\n\tb: [\n\t\ttrue ], z: [\n\t\t\t1\n\t\t],}\n",
	);
}

#[test]
fn set_into_an_empty_container_indents_the_new_entry_like_the_lines_of_its_value() {
	// The closing bracket's line is indented more than the container's line.
	check(
		"{\n\t}",
		|document| document.set(["n", "m"], 1).expect("set"),
		"{\n\tn: {\n\t\tm: 1\n\t}\n}",
	);
	check(
		"a: [\n\t]\n",
		|document| {
			document
				.set([PathSegment::from("a"), 0.into()], vec![1, 2])
				.expect("set")
		},
		"a: [\n\t[\n\t\t1\n\t\t2\n\t]\n]\n",
	);
	check(
		"[ # o\n\t]",
		|document| document.set([0], Value::Null).expect("set"),
		"[ # o\n\tnull\n]",
	);

	// The closing bracket's line is indented less than the container's line.
	check(
		"a:\n\t{\n}\n",
		|document| document.set(["a", "n"], vec![1]).expect("set"),
		"a:\n\t{\n\t\tn: [\n\t\t\t1\n\t\t]\n\t}\n",
	);
	check(
		"a:\n\t{\n\t # c\n}\n",
		|document| document.set(["a", "n"], 1).expect("set"),
		"a:\n\t{\n\t # c\n\t\tn: 1\n\t}\n",
	);
}

/**
Documents with comments in odd places, one-line and multi-line containers, block strings, and indentation, for the model test below.
*/
const MODEL_DOCUMENTS: [&str; 6] = [
	"# Head\nname: 'api' # Name.\nserver: {\n\thost: 'a'\n\tport: 0x1F90\n}\n\n# Lists\nhosts: ['a', /* b */ 'b']\nlimits: {cpu: 1, memory: 2}\n",
	"{\n\ta: {\n\t\tb: 1, # one\n\t\tc: [1, 2],\n\t},\n\t/* d */ d: '''\n\t\ttext\n\t\t''',\n}\n",
	"[\n\t{a: 1},\n\t[1, 2, 3],\n\t# c\n\t'x',\n]",
	"  a: 1\n  b: {c: {d: []}}\n  e: { /* empty */ }",
	"x: {y: {z: 1}} # end",
	"list: [\n    1,\n    2\n]\nobject: {\n    k: 'v'\n}\n",
];

/**
The paths to every value in a value.
*/
fn model_paths(
	value: &Value,
	prefix: &mut Vec<PathSegment<'static>>,
	output: &mut Vec<Vec<PathSegment<'static>>>,
) {
	match value {
		Value::Object(object) => {
			for (key, member) in object {
				prefix.push(PathSegment::Key(Box::leak(key.clone().into_boxed_str())));
				output.push(prefix.clone());
				model_paths(member, prefix, output);
				prefix.pop();
			}
		}
		Value::Array(items) => {
			for (index, item) in items.iter().enumerate() {
				prefix.push(PathSegment::Index(index));
				output.push(prefix.clone());
				model_paths(item, prefix, output);
				prefix.pop();
			}
		}
		_ => {}
	}
}

fn model_at<'a>(value: &'a mut Value, path: &[PathSegment<'_>]) -> Option<&'a mut Value> {
	path.iter().try_fold(value, |value, segment| match segment {
		PathSegment::Key(key) => value.get_mut(*key),
		PathSegment::Index(index) => value.get_mut(*index),
	})
}

/**
A change to a formatted document leaves it formatted, as the spec's editing rules make sure. Every value of each formatted corpus case, up to a limit, is removed, replaced, and given a new sibling, one change at a time.
*/
#[test]
fn a_change_to_a_formatted_corpus_case_leaves_it_formatted() {
	let mut failures = Vec::new();

	for (name, _) in corpus() {
		let formatted = fs::read_to_string(name.replace(".soml", ".formatted.soml"))
			.expect("the formatted form exists");
		let value = document(&formatted).to_value().expect("valid");
		let mut paths = Vec::new();
		model_paths(&value, &mut Vec::new(), &mut paths);

		for path in paths.into_iter().take(40) {
			let (last, parent) = path.split_last().expect("a path is not empty");
			let mut sibling = parent.to_vec();
			sibling.push(match last {
				PathSegment::Key(_) => PathSegment::Key("added"),
				PathSegment::Index(_) => PathSegment::Index(
					model_at(&mut value.clone(), parent)
						.and_then(|parent| parent.as_array().map(Vec::len))
						.expect("an array"),
				),
			});

			for label in ["remove", "replace", "add"] {
				let mut document = document(&formatted);
				let result = match label {
					"remove" => document.remove(path.clone()).map(|_| ()),
					"replace" => document.set(path.clone(), Value::Array(vec![Value::Int(1)])),
					_ => document.set(sibling.clone(), 1),
				};

				if result.is_err() {
					continue;
				}

				let printed = document.to_string();

				if soml::format(&printed).ok().as_ref() != Some(&printed) {
					failures.push(format!("{name}: {label} {path:?} gives {printed:?}"));
				}
			}
		}
	}

	assert!(
		failures.is_empty(),
		"{} failures:\n{}",
		failures.len(),
		failures.join("\n")
	);
}

proptest::proptest! {
	#![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]

	/**
	Random changes keep the document valid, give the value the same changes give a `Value`, and leave text that prints back unchanged.
	*/
	#[test]
	fn random_changes_match_the_same_changes_on_a_value(
		document_index in 0..MODEL_DOCUMENTS.len(),
		steps in proptest::collection::vec((0..3u8, proptest::prelude::any::<proptest::sample::Index>(), -5..5i64), 1..6),
	) {
		let mut document = document(MODEL_DOCUMENTS[document_index]);
		let mut expected = document.to_value().expect("valid");

		for (operation, choice, number) in steps {
			let mut paths = Vec::new();
			model_paths(&expected, &mut Vec::new(), &mut paths);

			if paths.is_empty() {
				break;
			}

			let path = choice.get(&paths).clone();

			match operation {
				// Replace a value that is there.
				0 => {
					if document.set(path.clone(), number).is_ok() {
						*model_at(&mut expected, &path).expect("the path is in the model") = Value::Int(number);
					}
				}
				// Add a member next to a value in an object.
				1 => {
					let (_, parent) = path.split_last().expect("a path is not empty");
					let mut new_path = parent.to_vec();
					new_path.push(PathSegment::Key("added"));

					if model_at(&mut expected, parent).and_then(|value| value.as_object()).is_some_and(|object| !object.contains_key("added")) && document.set(new_path, number).is_ok() {
						model_at(&mut expected, parent).and_then(Value::as_object_mut).expect("an object").insert("added".to_owned(), Value::Int(number));
					}
				}
				// Remove a value.
				_ => {
					if document.remove(path.clone()).expect("the path is valid") {
						let (last, parent) = path.split_last().expect("a path is not empty");

						match (model_at(&mut expected, parent).expect("the parent is in the model"), last) {
							(Value::Object(object), PathSegment::Key(key)) => {
								object.remove(*key);
							}
							(Value::Array(items), PathSegment::Index(index)) => {
								items.remove(*index);
							}
							_ => unreachable!("the path came from the model"),
						}
					}
				}
			}

			let printed = document.to_string();
			let value: Value = printed.parse().map_err(|error| proptest::test_runner::TestCaseError::fail(format!("{error}\n{printed}")))?;
			proptest::prop_assert_eq!(&value, &expected, "{}", printed);
			proptest::prop_assert_eq!(printed.parse::<Document>().expect("valid").to_string(), printed);
		}
	}
}

#[test]
fn a_new_node_holds_at_most_99_levels_because_it_is_always_inside_a_collection() {
	fn nested(levels: usize) -> Value {
		(0..levels).fold(Value::Int(1), |value, _| Value::Array(vec![value]))
	}

	assert!(Node::new(nested(99)).is_ok());
	assert!(soml::tree::Item::new(nested(99)).is_ok());
	assert!(Member::new(Key::new("a"), nested(99)).is_ok());

	assert_eq!(
		Node::new(nested(100)).unwrap_err().kind(),
		soml::ErrorKind::Write
	);
	assert!(soml::tree::Item::new(nested(100)).is_err());
	assert!(Member::new(Key::new("a"), nested(100)).is_err());
}

#[test]
fn outer_decor_is_the_text_around_a_braced_top_level_collection() {
	assert_eq!(
		document("# a\n[1] # b\n").outer_decor(),
		&Decor {
			leading: "# a\n".to_owned(),
			trailing: " # b\n".to_owned(),
		}
	);
	// A top-level object without braces has no text around it: its members hold the comments.
	assert_eq!(document("# a\nx: 1 # b\n").outer_decor(), &Decor::default());
}

#[test]
fn remove_keeps_the_line_feed_after_an_entry_that_shared_its_line() {
	check(
		"x: [\n\t1, 2,\n\t3,\n]",
		|document| {
			assert!(
				document
					.remove([PathSegment::from("x"), 1.into()])
					.expect("remove")
			)
		},
		"x: [\n\t1,\n\t3,\n]",
	);
	check(
		"x: {\n\ta: 1, b: 2,\n}",
		|document| assert!(document.remove(["x", "b"]).expect("remove")),
		"x: {\n\ta: 1,\n}",
	);
}

#[test]
fn remove_takes_a_blank_line_it_would_leave_next_to_another_or_at_an_edge() {
	for (before, path, after) in [
		("a: 1\n\nb: 2\n\nc: 3\n", vec!["b"], "a: 1\n\nc: 3\n"),
		("a: 1\n\nb: 2\n", vec!["b"], "a: 1\n"),
		("a: 1\n\nb: 2\n", vec!["a"], "b: 2\n"),
		// Blank lines on both sides of a removed first member, one of them holding a tab. The one after it stays, as written, as in the JavaScript reference.
		("\n\ta: 1\n\t\nb: 2\n", vec!["a"], "\t\nb: 2\n"),
		// A removal takes at most one blank line, so the author's other blank lines stay, also at an edge.
		("\na: 1\nb: 2\n", vec!["a"], "\nb: 2\n"),
		("\n\na: 1\nb: 2\n", vec!["a"], "\n\nb: 2\n"),
		("a: 1\n\nb: 2\n\n", vec!["b"], "a: 1\n\n"),
		("a: 1\n\nb: 2\n\t\n", vec!["b"], "a: 1\n\t\n"),
		(
			"{\n\n\ta: 1\n\t\n\tb: 2\n}\n",
			vec!["a"],
			"{\n\t\n\tb: 2\n}\n",
		),
		(
			"x: {\n\ta: 1,\n\n\tb: 2,\n}",
			vec!["x", "b"],
			"x: {\n\ta: 1,\n}",
		),
		(
			"x: {\n\ta: 1,\n\n\tb: 2,\n}",
			vec!["x", "a"],
			"x: {\n\tb: 2,\n}",
		),
		// Blank lines next to a comment stay, and so do blank lines away from the removed entry.
		(
			"a: 1\n\n# Note.\nb: 2\n\nc: 3\n",
			vec!["b"],
			"a: 1\n\n# Note.\n\nc: 3\n",
		),
		("\n# Note.\na: 1\nb: 2\n", vec!["a"], "\n# Note.\nb: 2\n"),
		// A comment on the bracket's line does not keep a blank line inside the bracket.
		(
			"a: { # c\n\tx: 1,\n\n\ty: 2,\n}\n",
			vec!["a", "x"],
			"a: { # c\n\ty: 2,\n}\n",
		),
		// The only top-level member, where `{}` takes its place.
		(
			"d: {a: 1} # End.\n\n/* c */",
			vec!["d"],
			"{} # End.\n\n/* c */",
		),
	] {
		check(
			before,
			|document| assert!(document.remove(path).expect("remove")),
			after,
		);
	}
}

#[test]
fn set_indents_a_new_or_replaced_entry_like_the_line_it_is_on() {
	check(
		"x: {\n\ta: 1, b: 2,\n}",
		|document| {
			document
				.set(["x", "c"], Value::Array(vec![Value::Int(1)]))
				.expect("set")
		},
		"x: {\n\ta: 1, b: 2,\n\tc: [\n\t\t1\n\t]\n}",
	);
	check(
		"x: {\n\ta: 1, b: 2,\n}",
		|document| {
			document
				.set(["x", "b"], Value::Array(vec![Value::Int(1)]))
				.expect("set")
		},
		"x: {\n\ta: 1, b: [\n\t\t1\n\t],\n}",
	);
	// A value on the line after its key, because of a comment, is indented like that line.
	check(
		"b: # c\n\t2",
		|document| document.set(["b"], soml::soml!({k: 1})).expect("set"),
		"b: # c\n\t{\n\t\tk: 1\n\t}",
	);
}

#[test]
fn an_int_knows_its_radix() {
	let document = document("a: [0b1010, 0o644, -42, 0xFF, 1.5]");
	let radixes: Vec<Option<Radix>> = (0..5)
		.map(|index| {
			document
				.get([PathSegment::from("a"), index.into()])
				.and_then(Node::as_scalar)
				.and_then(soml::tree::Scalar::radix)
		})
		.collect();
	assert_eq!(
		radixes,
		[
			Some(Radix::Binary),
			Some(Radix::Octal),
			Some(Radix::Decimal),
			Some(Radix::Hexadecimal),
			None,
		]
	);
}

#[test]
fn an_entry_added_after_the_last_line_ends_that_line_itself() {
	// Without a final line feed, the line feed that ends the old last line belongs to it, so removing it later leaves no blank line.
	check(
		"a: 1\nb: 2",
		|document| {
			document.set(["c"], 9).expect("set");
			assert!(document.remove(["b"]).expect("remove"));
		},
		"a: 1\nc: 9",
	);
	// The same in a container whose closing bracket is on the line of its last item, which is not a container written on one line.
	check(
		"x: [\n\t1,\n\t2]",
		|document| {
			document
				.set([PathSegment::from("x"), 2.into()], 3)
				.expect("set");
			assert!(
				document
					.remove([PathSegment::from("x"), 1.into()])
					.expect("remove")
			);
		},
		"x: [\n\t1,\n\t3]",
	);
}

#[test]
fn set_puts_a_new_entry_on_the_line_of_the_entry_before_it_when_something_follows_that_entry() {
	// As in the JavaScript reference: a closing bracket on the line of the last entry keeps a new entry on that line, after a comma.
	check(
		"x: {\n\ta: 1, b: 2}",
		|document| document.set(["x", "c"], 3).expect("set"),
		"x: {\n\ta: 1, b: 2, c: 3}",
	);
	check(
		"x: [\n\t1,\n\t2]",
		|document| {
			document
				.set([PathSegment::from("x"), 2.into()], 3)
				.expect("set");
		},
		"x: [\n\t1,\n\t2, 3]",
	);
	check(
		"x: {a: 1, b: 2}",
		|document| document.set(["x", "c"], 3).expect("set"),
		"x: {a: 1, b: 2, c: 3}",
	);
	// Without anything after the entry before it on its line, the new entry goes on a line of its own, with no comma.
	check(
		"x: {\n\ta: 1,\n\tb: 2\n}",
		|document| document.set(["x", "c"], 3).expect("set"),
		"x: {\n\ta: 1,\n\tb: 2\n\tc: 3\n}",
	);
}

/**
Applies `changes` one at a time to the same document, and again to a document parsed anew from the text after each change, and checks that both give the same text, which is `after`.
*/
fn check_same_as_reparsed(before: &str, changes: &[&dyn Fn(&mut Document)], after: &str) {
	let mut kept = document(before);
	let mut reparsed = document(before);

	for change in changes {
		change(&mut kept);
		change(&mut reparsed);
		reparsed = document(&reparsed.to_string());
	}

	assert_eq!(reparsed.to_string(), after, "reparsed");
	assert_eq!(kept.to_string(), after, "kept");
}

#[test]
fn a_change_gives_the_same_text_as_on_the_document_parsed_again() {
	let remove = |path: Vec<PathSegment<'static>>| {
		move |document: &mut Document| {
			document.remove(path.clone()).expect("remove");
		}
	};

	// The line feed after an entry that shared a line with the one before it stays with that one.
	check_same_as_reparsed(
		"a: [\n\t1,\n\t7, 2,\n]",
		&[
			&remove(vec!["a".into(), 2.into()]),
			&remove(vec!["a".into(), 1.into()]),
		],
		"a: [\n\t1,\n]",
	);
	check_same_as_reparsed(
		"e: [1, 2,\n\t3,\n]",
		&[
			&remove(vec!["e".into(), 1.into()]),
			&remove(vec!["e".into(), 0.into()]),
		],
		"e: [\n\t3,\n]",
	);

	// A comment after the comma of the last entry, with the closing bracket after it on its line, belongs to no entry, so a new entry goes before it.
	check_same_as_reparsed(
		"a: {\n\tx: 1, /* c */}\n",
		&[&|document: &mut Document| document.set(["a", "y"], 2).expect("set")],
		"a: {\n\tx: 1, y: 2, /* c */}\n",
	);

	// The text between a value and a comma that goes is after the value.
	check_same_as_reparsed(
		"{\n\tb: 's' /* c */ , d: []\n}\n",
		&[&remove(vec!["d".into()]), &|document: &mut Document| {
			document.set(["fresh"], 1).expect("set")
		}],
		"{\n\tb: 's' /* c */\n\tfresh: 1\n}\n",
	);
}

#[test]
fn remove_keeps_a_comment_before_a_closing_bracket_after_the_comma_of_the_last_entry() {
	// The comment is after the comma, and the bracket follows it on its line, so it belongs to no entry, and it takes the removed entry's place on its line.
	check(
		"{\n\tw: 0,\n\tx: 1, /* c */}",
		|document| assert!(document.remove(["x"]).expect("remove")),
		"{\n\tw: 0,\n\t/* c */}",
	);
}

#[test]
fn the_comments_after_the_last_entry_on_its_line_are_its_own() {
	let item = |index: usize| [PathSegment::from("a"), index.into()];

	check(
		"a: [1, 2 /* c */]",
		|document| assert!(document.remove(item(1)).expect("remove")),
		"a: [1]",
	);
	check(
		"a: [1 /* c */]",
		|document| document.set(item(1), 2).expect("set"),
		"a: [1 /* c */, 2]",
	);
	check(
		"a: [1 /* c */ ]",
		|document| document.set(item(1), 2).expect("set"),
		"a: [1 /* c */, 2 ]",
	);
	check(
		"a: [\n\t1\n\t2 # c\n]",
		|document| document.set(item(2), 3).expect("set"),
		"a: [\n\t1\n\t2 # c\n\t3\n]",
	);
	// A block comment that spans lines is on the line it starts on.
	check(
		"a: [1, 2 /* m\nl */]",
		|document| document.set(item(2), 3).expect("set"),
		"a: [1, 2 /* m\nl */, 3]",
	);
	check(
		"a: [\n\t1 /* m\n\tl */\n\t2 /* m\n\tl */\n]",
		|document| assert!(document.remove(item(1)).expect("remove")),
		"a: [\n\t1 /* m\n\tl */\n]",
	);
}

#[test]
fn the_comments_before_an_entry_on_its_line_are_its_own() {
	let item = |index: usize| [PathSegment::from("a"), index.into()];

	// The comma before the last entry goes with it, also across the comments it owns.
	check(
		"a: [1, /* c */ 2]",
		|document| assert!(document.remove(item(1)).expect("remove")),
		"a: [1]",
	);
	check(
		"a: [1, /* m\nl */ 2]",
		|document| assert!(document.remove(item(1)).expect("remove")),
		"a: [1]",
	);
	check(
		"a: [/* c */ 1, 2]",
		|document| assert!(document.remove(item(0)).expect("remove")),
		"a: [2]",
	);
	check(
		"a: [/* c */ 1]",
		|document| assert!(document.remove(item(0)).expect("remove")),
		"a: []",
	);
	check(
		"/* c */ a: 1\nb: 2",
		|document| assert!(document.remove(["a"]).expect("remove")),
		"b: 2",
	);
	// A comment on a line of its own belongs to no entry.
	check(
		"a: [\n\t1\n\t# About 2\n\t2\n]",
		|document| assert!(document.remove(item(1)).expect("remove")),
		"a: [\n\t1\n\t# About 2\n]",
	);
}

#[test]
fn a_comment_before_a_comma_that_goes_stays_with_its_entry() {
	let remove = |path: Vec<PathSegment<'static>>| {
		move |document: &mut Document| {
			document.remove(path.clone()).expect("remove");
		}
	};

	check_same_as_reparsed(
		"{a: 1 /* c */, b: 2}",
		&[&remove(vec!["b".into()]), &|document: &mut Document| {
			document.set(["z"], 3).expect("set")
		}],
		"{a: 1 /* c */, z: 3}",
	);
	check_same_as_reparsed(
		"{a: 1 /* c */, b: 2}",
		&[&remove(vec!["b".into()]), &remove(vec!["a".into()])],
		"{}",
	);
}

#[test]
fn entries_added_on_a_shared_line_keep_the_commas_they_need() {
	check_same_as_reparsed(
		"x: {\n\tp: 0, b: 9}",
		&[
			&|document: &mut Document| document.set(["x", "c"], 1).expect("set"),
			&|document: &mut Document| document.set(["x", "d"], 2).expect("set"),
		],
		"x: {\n\tp: 0, b: 9, c: 1, d: 2}",
	);
}

#[test]
fn a_path_error_names_the_path_with_keys_and_indexes() {
	let mut document = document("a: [{b: 1}]\nc: [1, 2]\n");
	let message = |result: Result<(), soml::Error>| result.unwrap_err().message().to_owned();

	assert_eq!(
		message(document.set(
			[PathSegment::from("a"), 0.into(), "b".into(), "c".into()],
			1
		)),
		"Cannot edit a[0].b.c, because a[0].b is not an object or an array"
	);
	assert_eq!(
		message(document.set([PathSegment::from("c"), 3.into()], 1)),
		"Cannot edit c[3], because the array at c has 2 items. Add an item at index 2"
	);
	// An index equal to the length appends, with new objects for the keys after it, but an index after it is on an array that does not exist.
	assert_eq!(
		message(document.set([PathSegment::from("c"), 2.into(), 0.into()], 1)),
		"Cannot edit c[2][0], because c[2] does not exist, so it has no index 0"
	);

	// A key that is not bare is a JSON string, as in the JavaScript reference.
	let mut document = self::document("'the key': 1\n");
	assert_eq!(
		message(document.set(["the key", "x.y"], 1)),
		"Cannot edit \"the key\".\"x.y\", because \"the key\" is not an object or an array"
	);
}

#[test]
fn a_path_error_has_the_text_of_the_javascript_reference() {
	use PathSegment::{Index, Key};

	// The messages of `edit()` in the JavaScript reference for the same document, path, and change, where `None` removes.
	let cases: &[(&str, &[PathSegment<'_>], Option<i64>, &str)] = &[
		(
			"a: 1",
			&[Key("a"), Key("b")],
			Some(1),
			"Cannot edit a.b, because a is not an object or an array",
		),
		(
			"a: 1",
			&[Key("a"), Index(0)],
			None,
			"Cannot edit a[0], because a is not an object or an array",
		),
		(
			"a: {b: 1}",
			&[Key("a"), Key("b"), Key("c"), Key("d")],
			Some(1),
			"Cannot edit a.b.c.d, because a.b is not an object or an array",
		),
		(
			"a: [1]",
			&[Key("a"), Index(0), Key("b")],
			None,
			"Cannot edit a[0].b, because a[0] is not an object or an array",
		),
		(
			"a: [1]",
			&[Key("a"), Key("b")],
			Some(1),
			"Cannot edit a.b, because a is an array, so it needs an index, not a key",
		),
		(
			"[1]",
			&[Key("a")],
			Some(1),
			"Cannot edit a, because the document is an array, so it needs an index, not a key",
		),
		(
			"a: {b: [1]}",
			&[Key("a"), Key("b"), Key("c")],
			None,
			"Cannot edit a.b.c, because a.b is an array, so it needs an index, not a key",
		),
		(
			"a: {}",
			&[Key("a"), Index(0)],
			Some(1),
			"Cannot edit a[0], because a is an object, so it needs a key, not an index",
		),
		(
			"a: 1",
			&[Index(0)],
			Some(1),
			"Cannot edit [0], because the document is an object, so it needs a key, not an index",
		),
		(
			"a: {b: 1}",
			&[Key("a"), Index(0)],
			None,
			"Cannot edit a[0], because a is an object, so it needs a key, not an index",
		),
		(
			"a: {b: {c: 1}}",
			&[Key("a"), Key("b"), Index(0), Key("x")],
			Some(1),
			"Cannot edit a.b[0].x, because a.b is an object, so it needs a key, not an index",
		),
		(
			"a: {b: {c: 1}}",
			&[Key("a"), Key("b"), Index(0), Key("x")],
			None,
			"Cannot edit a.b[0].x, because a.b is an object, so it needs a key, not an index",
		),
		(
			"a: [1]",
			&[Key("a"), Index(3)],
			Some(1),
			"Cannot edit a[3], because the array at a has 1 item. Add an item at index 1",
		),
		(
			"a: []",
			&[Key("a"), Index(3)],
			Some(1),
			"Cannot edit a[3], because the array at a has 0 items. Add an item at index 0",
		),
		(
			"[1, 2]",
			&[Index(5)],
			Some(1),
			"Cannot edit [5], because the document has 2 items. Add an item at index 2",
		),
		(
			"a: 1",
			&[Key("b"), Index(0)],
			Some(1),
			"Cannot edit b[0], because b does not exist, so it has no index 0",
		),
		(
			"a: 1",
			&[Key("b"), Index(0), Index(1)],
			Some(1),
			"Cannot edit b[0][1], because b[0] does not exist, so it has no index 1",
		),
		(
			"a: 1",
			&[Key("b"), Key("c"), Index(2), Key("d")],
			Some(1),
			"Cannot edit b.c[2].d, because b.c does not exist, so it has no index 2",
		),
		(
			"a: [1]",
			&[Key("a"), Index(1), Index(0), Index(3)],
			Some(1),
			"Cannot edit a[1][0][3], because a[1][0] does not exist, so it has no index 3",
		),
		(
			"a: [1]",
			&[Key("a"), Index(1), Key("x"), Index(0), Key("y")],
			Some(1),
			"Cannot edit a[1].x[0].y, because a[1].x does not exist, so it has no index 0",
		),
		(
			"a: {b: 1}",
			&[Key("a"), Key("c"), Index(0), Index(1)],
			Some(1),
			"Cannot edit a.c[0][1], because a.c[0] does not exist, so it has no index 1",
		),
		(
			"[1]",
			&[Index(1), Index(0), Key("k"), Index(5)],
			Some(1),
			"Cannot edit [1][0].k[5], because [1][0].k does not exist, so it has no index 5",
		),
		(
			"'the key': 1",
			&[Key("the key"), Key("x.y")],
			Some(1),
			r#"Cannot edit "the key"."x.y", because "the key" is not an object or an array"#,
		),
		// The characters that a terminal acts on, and invisible ones, are escapes.
		(
			"a: 1",
			&[Key("a"), Key("\u{9B}2J\u{202E}")],
			Some(1),
			r#"Cannot edit a."\u{9b}2J\u{202e}", because a is not an object or an array"#,
		),
		(
			"a: 1",
			&[Key("a"), Key("\u{7F}\n\"\\")],
			Some(1),
			r#"Cannot edit a."\u{7f}\n\"\\", because a is not an object or an array"#,
		),
		(
			"a: 1",
			&[Key("a"), Key("x\u{2028}y\u{AD}"), Key("z")],
			Some(1),
			r#"Cannot edit a."x\u{2028}y\u{ad}".z, because a is not an object or an array"#,
		),
		(
			"a: 1",
			&[],
			Some(1),
			"The path must be a non-empty array of keys and array indexes",
		),
		(
			"a: 1",
			&[],
			None,
			"The path must be a non-empty array of keys and array indexes",
		),
	];

	for &(text, path, value, expected) in cases {
		let mut document = document(text);
		let error = match value {
			Some(value) => document.set(path.iter().copied(), value).unwrap_err(),
			None => document.remove(path.iter().copied()).unwrap_err(),
		};
		assert_eq!(error.message(), expected, "{text:?} {path:?}");
	}

	// A long path is cut short, as each long key in it is.
	let path = vec![PathSegment::from("a"); 1000];
	let message = document("a: 1")
		.set(path, 1)
		.unwrap_err()
		.message()
		.to_owned();
	assert_eq!(
		message,
		format!(
			"Cannot edit {}a.…, because a is not an object or an array",
			"a.".repeat(99)
		)
	);
	let key = "k".repeat(50);
	let message = document("k: 1")
		.set([PathSegment::from("k"), Key(&key)], 1)
		.unwrap_err()
		.message()
		.to_owned();
	assert_eq!(
		message,
		format!(
			"Cannot edit k.{}…, because k is not an object or an array",
			"k".repeat(40)
		)
	);
}

#[test]
fn get_finds_nothing_past_the_end_of_an_array_or_through_a_scalar() {
	let document = document("a: [1, 2]\nb: 1\nc: {d: 2}\n");
	assert!(document.get([PathSegment::from("a"), 1.into()]).is_some());
	assert!(document.get([PathSegment::from("a"), 2.into()]).is_none());
	assert!(
		document
			.get([PathSegment::from("a"), usize::MAX.into()])
			.is_none()
	);
	assert!(document.get([PathSegment::from("a"), "x".into()]).is_none());
	assert!(document.get([PathSegment::from("b"), 0.into()]).is_none());
	assert!(document.get(["b", "x"]).is_none());
	assert!(document.get([PathSegment::Index(0)]).is_none());
	assert!(document.get(["c", "d"]).is_some());
	assert!(document.get(["c", "e"]).is_none());
}
