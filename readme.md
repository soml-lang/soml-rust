# soml-lang

> [SOML](https://soml.sh) for Rust: a strict reader, serde support, and a lossless syntax tree

> [!NOTE]
> SOML is a config format for humans. The format is a draft.

## Highlights

- Every rule in the spec is checked: duplicate keys, the int64 range, instant and duration ranges, raw control characters, the nesting limit, and more
- Errors give a line and a column, also when a valid document does not fit your type
- Writes members in the order of your fields, or in canonical form, where the same value always gives the same bytes
- A lossless syntax tree that keeps comments and the author's spelling when you change a document
- A formatter that changes layout and nothing else
- `Spanned<T>` for the position of any value, and error messages with a code frame
- `std::time::Duration` reads and writes SOML durations. `jiff::Timestamp`, `jiff::SignedDuration`, and `chrono::DateTime<Utc>` fields read SOML instants and durations, and the optional `jiff` and `chrono` features write them as native values
- Passes the language-neutral [conformance suite](https://github.com/soml-lang/soml/tree/main/conformance) of the spec
- No unsafe code, and two required dependencies: `serde_core` and `zmij`

## Install

```sh
cargo add soml-lang
```

The package is `soml-lang`, and the library is `soml`.

## Usage

[API documentation](https://docs.rs/soml-lang)

### Read

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
	postgres: Postgres,
}

#[derive(Deserialize)]
struct Postgres {
	host: String,
	port: u16,
}

fn main() -> Result<(), soml::Error> {
	let config: Config = soml::from_str("
		/* The edge service. */
		name: 'api-gateway'
		replicas: 3
		timeout: 1m30s # Including retries.
		deployed-at: 2026-09-19T14:00:00Z
		labels: ['prod', 'eu-west']
		postgres: {host: 'db.internal', port: 5432}
	")?;

	assert_eq!(config.postgres.port, 5432);
	assert_eq!(config.timeout.as_secs(), 90);
	Ok(())
}
```

Indentation never matters in SOML, so a document can be indented like the code around it.

### Write

```rust
#[derive(serde::Serialize)]
struct Server {
	port: u16,
	hosts: Vec<&'static str>,
}

fn main() -> Result<(), soml::Error> {
	let server = Server { port: 8080, hosts: vec!["a", "b"] };
	assert_eq!(soml::to_string(&server)?, "port: 8080\nhosts: [\n\t'a'\n\t'b'\n]\n");
	assert_eq!(soml::to_string_canonical(&server)?, "hosts: [\n\t'a'\n\t'b'\n]\nport: 8080\n");
	Ok(())
}
```

`soml::to_string` writes one member or item per line, tab indentation, and no commas, because a line break separates members and items. Members keep the order the value gives them: a struct's fields in the order they are declared, and a map's entries in the order it iterates in.

`soml::to_string_canonical` writes canonical form, which is the same with members sorted by key, so the same value always gives the same bytes. Use it to hash, sign, or compare documents, and to write a `HashMap`, which iterates in a different order on each run.

### Values of unknown shape

```rust
fn main() -> Result<(), soml::Error> {
	let value: soml::Value = "port: 8080\nhosts: ['a', 'b']".parse()?;

	assert_eq!(value.get("port").and_then(soml::Value::as_i64), Some(8080));
	assert_eq!(value.get("hosts").and_then(|hosts| hosts.get(1)).and_then(soml::Value::as_str), Some("b"));
	Ok(())
}
```

An int and a float are different types, as in the spec, so `as_f64()` does not convert an int.

### Change a document and keep its comments

```rust
fn main() -> Result<(), soml::Error> {
	let mut document: soml::Document = "/* The edge service. */\nport: 0x1F90 # The default.\nserver: {host: 'a'}\n".parse()?;

	document.set(["port"], 8080)?;
	document.set(["server", "port"], 443)?;
	document.set(["replicas"], 3)?;

	assert_eq!(document.to_string(), "/* The edge service. */\nport: 8080 # The default.\nserver: {host: 'a', port: 443}\nreplicas: 3\n");
	Ok(())
}
```

An unchanged document prints the exact text it was parsed from. A changed one keeps the comments, blank lines, indentation, member order, and the author's spelling of every value outside the change. Changes follow the spec's editing rules, as in the JavaScript reference: a new member goes after the last member of its object, a removed member or item takes the comments it owns, such as one after it on its line, and a change to a formatted document leaves it formatted. Removing a value that does not exist changes nothing and is not an error, so removing the same path twice is safe, and `remove` returns whether it removed something. The tree also gives every parsed node its position, for linters and editors.

### Format

```rust
fn main() -> Result<(), soml::Error> {
	assert_eq!(soml::format("a: {b: 1, c: [2,3]}  # Note.")?, "a: {b: 1, c: [2, 3]} # Note.\n");
	assert_eq!(soml::format("a: {\nb: 1, c: [2,3]}")?, "a: {\n\tb: 1\n\tc: [2, 3]\n}\n");
	Ok(())
}
```

The formatter changes layout and nothing else: one tab of indentation per level, every member and item on its own line, no commas, at most one blank line in a row, and no trailing spaces or tabs. An object or an array whose brackets are on one line stays on one line, with a comma and a space between its members or items. To give it one member or item per line, put a line break anywhere inside it. Comments stay where they are, and so do member order, block strings, and the spelling of every value. Formatting a formatted document gives the same text. The spec's formatter rules are normative, so every conforming formatter gives the same text, and the `tree` module lists them in detail.

### Errors

```rust
#[derive(serde::Deserialize, Debug)]
struct Server {
	port: u16,
}

let text = "host: 'a'\nport: 70000";
let error = soml::from_str::<Server>(text).unwrap_err();

assert_eq!(error.to_string(), "invalid value: integer `70000`, expected u16 at line 2, column 7");
assert_eq!(error.kind(), soml::ErrorKind::Data);

// For a terminal:
assert_eq!(error.code_frame(text).unwrap(), "  1 | host: 'a'\n> 2 | port: 70000\n    |       ^");
```

An error has a kind: `Syntax` for text that is not valid SOML, `Data` for a valid document that does not fit your type or a path that does not fit a `Document`, `Write` for a value that SOML cannot represent or a `Document::set` that would make the document invalid, and `Io` for reading and writing.

### Positions of values

```rust
#[derive(serde::Deserialize)]
struct Server {
	port: soml::Spanned<u16>,
}

fn main() -> Result<(), soml::Error> {
	let text = "port: 0";
	let server: Server = soml::from_str(text)?;

	if *server.port == 0 {
		let start = server.port.span().unwrap().start;
		let error = soml::Error::with_position("Port 0 is reserved", text, start);
		assert_eq!(error.to_string(), "Port 0 is reserved at line 1, column 7");
	}

	Ok(())
}
```

`Spanned<T>` reads any value with the byte range it came from, so checks your app makes after reading can point at the right place. The span is `None` for a value from `from_value` or from another format, for a map key, and where serde reads through its buffer: inside `#[serde(flatten)]`, untagged enums, internally tagged enums, and some adjacently tagged ones (see Limitations).

### Build a value

```rust
let value = soml::soml!({
	name: "api",
	"deployed-at": soml::Instant::from_unix(1_789_826_400, 0),
	replicas: 3,
	tags: ["prod", "eu-west"],
	owner: null,
});

assert_eq!(value.get("replicas").and_then(soml::Value::as_i64), Some(3));
```

A key is an identifier or a string literal, and a value is `null`, `{…}`, `[…]`, or any expression that converts to a `Value`. A key that is there twice panics.

### Readers and writers

`soml::from_reader`, `soml::to_writer`, and `soml::to_writer_canonical` read from an `io::Read` and write to an `io::Write`. A document is only valid as a whole, so `from_reader` reads everything first.

## Types

| Rust | SOML | Notes |
|---|---|---|
| `bool` | bool | |
| `i8`…`i128`, `u8`…`u128` | int | Range-checked both ways. An int is 64-bit, so a larger value cannot be written. |
| `f32`, `f64` | float | An int reads into a float only when it converts exactly. A float never reads into an int. An `f32` is written with its own shortest digits, so `0.1f32` is `0.1`, and a float too large for it reads as infinity, as in serde_json. NaN cannot be written. |
| `String`, `&str`, `char` | string | A `&str` borrows from the input when the string is `'...'` or `"..."` without escapes. Use `String` or `Cow<str>` when a document may hold other strings. |
| `Option<T>` | `null` or the value | A missing field reads as `None`. |
| `()`, a unit struct | `null` | |
| `Vec<T>`, a tuple | array | |
| a struct, `HashMap`, `BTreeMap` | object | A map key can be a string, a char, a bool, an integer (`404: 'x'`), or a unit enum variant. |
| an enum | string, or a one-member object | A unit variant is a string. The others are `{variant: value}`. |
| `soml::Instant` | instant | Exact, with nanoseconds. |
| `soml::Duration`, `std::time::Duration` | duration | A negative duration does not fit a `std::time::Duration`. |
| `soml::Value` | any value | |

Field names come from serde, so use `#[serde(rename_all = "kebab-case")]` for the usual SOML key style.

### Instants and durations from other crates

`jiff::Timestamp`, `jiff::SignedDuration`, `chrono::DateTime<Utc>`, and `humantime` fields read SOML instants and durations, because they ask for text, and an instant or a duration gives its canonical text. The cost of this is that a `String` field also accepts an instant or a duration. `chrono::TimeDelta` does not ask for text, so it needs `soml::chrono::time_delta` to read a duration too.

Those types write themselves as strings, so a document would get `'2026-09-19T14:00:00Z'` in quotes. To write native instants and durations, use `soml::Instant` and `soml::Duration`, or turn on a feature and use its module with `#[serde(with)]`:

```sh
cargo add soml-lang --features jiff
```

```rust
#[derive(serde::Serialize, serde::Deserialize)]
struct Deploy {
	#[serde(with = "soml::jiff::timestamp")]
	at: jiff::Timestamp,
	#[serde(with = "soml::jiff::signed_duration::option")]
	took: Option<jiff::SignedDuration>,
}
```

| Feature | Modules | Conversions |
|---|---|---|
| `jiff` | `soml::jiff::timestamp`, `soml::jiff::signed_duration` | `Timestamp` and `SignedDuration` to and from `soml::Instant` and `soml::Duration` |
| `chrono` | `soml::chrono::date_time`, `soml::chrono::time_delta` | `DateTime<Utc>` and `TimeDelta` to and from `soml::Instant` and `soml::Duration` |

Each module has an `option` module inside for an `Option`. The modules read only native values, not strings, except where serde reads through its buffer (see Limitations), and a value outside SOML's range is an error. jiff's timestamps end at `9999-12-30T22:00:00.999999999Z`, about a day before SOML's, so a later instant does not fit a `jiff::Timestamp`.

## Format

Documents follow the [SOML specification](https://soml.sh).

- File extension: `.soml`
- Encoding: UTF-8 without a byte order mark, with LF line endings
- Media type: `application/soml`
- Uniform type identifier: `com.sindresorhus.soml`

## Performance

The same 20,000 config-shaped records on an Apple M4 Pro, in a release build. Each format's own text is used, so the byte counts differ.

| | Read into a struct | Read into a value type | Write |
|---|---|---|---|
| soml-lang | 470 MB/s | 340 MB/s | 370 MB/s |
| serde_json | 990 MB/s | 580 MB/s | 1370 MB/s (pretty) |
| toml | 140 MB/s | 120 MB/s | 260 MB/s |

Reading builds a tree first, so that a document is checked as a whole before any of it is read into a type, and writing builds a tree first too. Both cost time that a JSON parser does not spend. The Write figure was measured when every write sorted its members, as `to_string_canonical` does now.

## Limitations

- `#[serde(flatten)]`, untagged enums, internally tagged enums, and adjacently tagged enums whose content member comes before the tag member read values through serde's buffer, which has no instant or duration type and no positions. `soml::to_string` writes the tag first, but `soml::to_string_canonical` puts the content first when its key sorts first, as with `#[serde(tag = "t", content = "c")]`. There, an instant or a duration is its text: a `soml::Value` holds it as a string, `soml::Instant` and `soml::Duration` still read it, and a `std::time::Duration` cannot be read. An int reads into a float even when the float does not hold it exactly, and a `Spanned` value has no span.
- An `f32` is read through an `f64`, as in serde_json, so 2 of the 4.3 billion `f32` values read back one step away from the value that was written.
- `std::time::Duration` is recognized by its serde shape, a struct named `Duration` with the fields `secs` and `nanos`, because serde gives no other type information. A struct of your own with that name and those fields is written as a duration too, and it reads back.
- `soml::Value` keeps object members sorted by key, because member order is not part of a value, so writing one always gives canonical order. `soml::Document` keeps the written order.
- The `soml!` macro reads one item or member at a time, so a literal with more than about 120 of them at one level reaches the compiler's recursion limit, as with `json!`.
- Nesting is limited to 100 levels, as the spec requires. Every object and array counts, including the document's own.
