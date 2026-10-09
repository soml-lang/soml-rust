/*
Compares soml-lang with serde_json and toml on the same data, and prints a Markdown table.

	cargo run --release --manifest-path bench/Cargo.toml

On macOS, `-- --instructions` prints the instructions that each operation takes instead of its time. A busy machine changes the time much, but the instructions only a little.

The fixtures are a copy of the shared ones in the soml repository, and `fixtures/readme.md` tells what each one measures and how to update them. Each fixture is in SOML, JSON, and TOML, with the same data. Each format reads its own text, so the sizes differ, and the cost of each operation is the number to compare.

With a mode, it runs one operation on one fixture instead, for a profiler or an instruction counter. The fixture is `earthquakes` unless `FIXTURE` names another:

	FIXTURE=service cargo run --release --manifest-path bench/Cargo.toml -- struct 1000
*/

#![allow(clippy::tabs_in_doc_comments)]

use serde::de::{self, DeserializeOwned, Deserializer, Visitor};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::{self, Debug};
use std::hint::black_box;
use std::time::{Duration, Instant};

/**
A fixture's text in each format, with the same data.
*/
struct Fixture {
	name: &'static str,
	soml: &'static str,
	json: &'static str,
	toml: &'static str,
}

macro_rules! fixture {
	($name:literal) => {
		Fixture {
			name: $name,
			soml: include_str!(concat!("../fixtures/", $name, ".soml")),
			json: include_str!(concat!("../fixtures/", $name, ".json")),
			toml: include_str!(concat!("../fixtures/", $name, ".toml")),
		}
	};
}

/**
A hand-written service config of 3 KB, with comments, block strings, durations, an instant, and hex, octal, and underscored ints.
*/
const SERVICE: Fixture = fixture!("service");

/**
A config of 35 KB in which a quarter of the lines are comments. It has no struct, because it has hundreds of settings.
*/
const PLATFORM: Fixture = fixture!("platform");

/**
986 earthquakes from the USGS feed as GeoJSON, the large document of 733 KB, with about 2,000 instants.
*/
const EARTHQUAKES: Fixture = fixture!("earthquakes");

const FIXTURES: [&Fixture; 3] = [&SERVICE, &PLATFORM, &EARTHQUAKES];

/**
A struct that a fixture is read into.
*/
trait Data: Serialize + DeserializeOwned + PartialEq + Debug {}

impl<T: Serialize + DeserializeOwned + PartialEq + Debug> Data for T {}

/**
The struct of a fixture, with the instant and duration types that a user of each format reads. JSON and TOML hold a duration as an ISO 8601 string, and JSON holds an instant as one too, which jiff reads.
*/
trait Types {
	type Soml: Data;
	type Json: Data;
	type Toml: Data;
	/**
	The TOML struct with instants that compare with those of the others, for the check that each format holds the same data.
	*/
	type TomlChecked: Data;
}

struct ServiceTypes;

impl Types for ServiceTypes {
	type Soml = Service<soml::Instant, soml::Duration>;
	type Json = Service<jiff::Timestamp, jiff::SignedDuration>;
	type Toml = Service<toml::value::Datetime, jiff::SignedDuration>;
	type TomlChecked = Service<TomlInstant, jiff::SignedDuration>;
}

struct EarthquakesTypes;

impl Types for EarthquakesTypes {
	type Soml = Earthquakes<soml::Instant>;
	type Json = Earthquakes<jiff::Timestamp>;
	type Toml = Earthquakes<toml::value::Datetime>;
	type TomlChecked = Earthquakes<TomlInstant>;
}

/**
A fixture read into the struct of each format.
*/
struct Structs<X: Types> {
	soml: X::Soml,
	json: X::Json,
	toml: X::Toml,
}

impl Fixture {
	/**
	Reads the fixture into its struct in each format. It checks that each format gives the same struct, or the comparison is not fair, and that each library reads back what it writes. An int in place of a float also gives the same struct, so that difference is left to the script that writes the fixtures, as is the data of `platform`, which has no struct.
	*/
	fn data<X: Types>(&self) -> Structs<X> {
		let data: X::Soml = soml::from_str(self.soml).unwrap();
		let json_data: X::Json = serde_json::from_str(self.json).unwrap();
		let toml_data: X::Toml = toml::from_str(self.toml).unwrap();

		// The SOML text read into the struct of each other format is the same data. jiff reads both ISO 8601 and the SOML text that soml-lang gives for a duration.
		assert_eq!(soml::from_str::<X::Json>(self.soml).unwrap(), json_data);
		assert_eq!(
			soml::from_str::<X::TomlChecked>(self.soml).unwrap(),
			toml::from_str::<X::TomlChecked>(self.toml).unwrap()
		);

		assert_eq!(
			soml::from_str::<X::Soml>(&soml::to_string(&data).unwrap()).unwrap(),
			data
		);
		assert_eq!(
			serde_json::from_str::<X::Json>(&serde_json::to_string_pretty(&json_data).unwrap())
				.unwrap(),
			json_data
		);
		assert_eq!(
			toml::from_str::<X::Toml>(&toml::to_string(&toml_data).unwrap()).unwrap(),
			toml_data
		);

		Structs {
			soml: data,
			json: json_data,
			toml: toml_data,
		}
	}
}

/**
An instant in the TOML struct for the check. The toml crate gives a TOML date-time as a map with one entry: a private key, and the date-time as a string. `soml::Instant` does not read that map, and `toml::value::Datetime` reads nothing else, so soml-lang cannot read the SOML text into it for the comparison.
*/
#[derive(Serialize, PartialEq, Debug)]
#[serde(transparent)]
struct TomlInstant(soml::Instant);

impl<'de> Deserialize<'de> for TomlInstant {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		struct TomlInstantVisitor;

		impl<'de> Visitor<'de> for TomlInstantVisitor {
			type Value = TomlInstant;

			fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
				formatter.write_str("a date-time")
			}

			fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<TomlInstant, A::Error> {
				let (_, text): (de::IgnoredAny, String) = map
					.next_entry()?
					.ok_or_else(|| de::Error::custom("a date-time"))?;
				text.parse().map(TomlInstant).map_err(de::Error::custom)
			}

			fn visit_str<E: de::Error>(self, text: &str) -> Result<TomlInstant, E> {
				text.parse().map(TomlInstant).map_err(E::custom)
			}
		}

		deserializer.deserialize_any(TomlInstantVisitor)
	}
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Service<I, D> {
	name: String,
	version: String,
	environment: String,
	region: String,
	deployed_at: I,
	server: Server<D>,
	database: Database<D>,
	cache: Cache<D>,
	queue: Queue<D>,
	payments: Payments<D>,
	limits: Limits,
	features: Features,
	logging: Logging,
	telemetry: Telemetry<D>,
	maintenance: Maintenance,
	regions: Vec<Region>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Server<D> {
	host: String,
	port: u16,
	keep_alive: D,
	read_timeout: D,
	write_timeout: D,
	max_header_size: u64,
	tls: Tls,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Tls {
	certificate: String,
	key: String,
	min_version: String,
	ciphers: Vec<String>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Database<D> {
	primary: String,
	replicas: Vec<String>,
	pool: Pool<D>,
	setup: String,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Pool<D> {
	min: u32,
	max: u32,
	idle_timeout: D,
	acquire_timeout: D,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Cache<D> {
	url: String,
	ttl: D,
	max_entries: u64,
	eviction: String,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Queue<D> {
	broker: String,
	exchanges: Vec<Exchange>,
	retry: Retry<D>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Exchange {
	name: String,
	r#type: String,
	durable: bool,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Retry<D> {
	attempts: u32,
	initial_delay: D,
	max_delay: D,
	multiplier: f64,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Payments<D> {
	provider: String,
	currency: String,
	capture: String,
	review_threshold: u64,
	fees: Fees,
	webhooks: Webhooks<D>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Fees {
	percentage: f64,
	fixed: u64,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Webhooks<D> {
	path: String,
	tolerance: D,
	events: Vec<String>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Limits {
	requests_per_minute: u32,
	burst: u32,
	max_body_size: u64,
	max_items_per_order: u32,
	cpu: f64,
	memory_mb: u32,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Features {
	express_checkout: bool,
	gift_cards: bool,
	buy_now_pay_later: bool,
	address_autocomplete: bool,
	new_tax_engine: bool,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Logging {
	level: String,
	format: String,
	sample_rate: f64,
	redact: Vec<String>,
	file: LogFile,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct LogFile {
	path: String,
	mode: u32,
	max_size_mb: u32,
	max_files: u32,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Telemetry<D> {
	endpoint: String,
	interval: D,
	attributes: BTreeMap<String, String>,
	histograms: Histograms,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Histograms {
	request_duration: Vec<f64>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Maintenance {
	enabled: bool,
	message: String,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct Region {
	name: String,
	weight: u32,
	primary: bool,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
struct Earthquakes<I> {
	r#type: String,
	metadata: Metadata<I>,
	bbox: [f64; 6],
	features: Vec<Earthquake<I>>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
struct Metadata<I> {
	generated: I,
	url: String,
	title: String,
	status: u16,
	api: String,
	count: u32,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
struct Earthquake<I> {
	r#type: String,
	properties: EarthquakeProperties<I>,
	geometry: Point,
	id: String,
}

/**
A member that the feed has as null is left out of each format, so it is an `Option`.
*/
#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EarthquakeProperties<I> {
	mag: f64,
	place: String,
	time: I,
	updated: I,
	url: String,
	detail: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	felt: Option<u32>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	cdi: Option<f64>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	mmi: Option<f64>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	alert: Option<String>,
	status: String,
	tsunami: u8,
	sig: u32,
	net: String,
	code: String,
	ids: String,
	sources: String,
	types: String,
	nst: u32,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	dmin: Option<f64>,
	rms: f64,
	gap: f64,
	mag_type: String,
	r#type: String,
	title: String,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(deny_unknown_fields)]
struct Point {
	r#type: String,
	coordinates: [f64; 3],
}

/**
What is measured for each operation.
*/
#[derive(Clone, Copy)]
enum Meter {
	Time,
	Instructions,
}

impl Meter {
	/**
	The cost of one run. Runs are measured in batches that take at least a millisecond, so that the clock's resolution does not matter for a small document. The time is the median of 50 batches, and the instructions are the fewest of 30.
	*/
	fn measure<R>(self, mut run: impl FnMut() -> R) -> String {
		let mut batch: u32 = 1;

		loop {
			let start = Instant::now();

			for _ in 0..batch {
				black_box(run());
			}

			if start.elapsed() >= Duration::from_millis(1) {
				break;
			}

			batch *= 2;
		}

		match self {
			Self::Time => {
				let mut samples: Vec<Duration> = (0..50)
					.map(|_| {
						let start = Instant::now();

						for _ in 0..batch {
							black_box(run());
						}

						start.elapsed() / batch
					})
					.collect();

				samples.sort_unstable();
				let seconds = samples[samples.len() / 2].as_secs_f64();

				if seconds < 1e-3 {
					format!("{} µs", significant(seconds * 1e6))
				} else {
					format!("{} ms", significant(seconds * 1e3))
				}
			}
			Self::Instructions => {
				// The fewest of 30 batches, because work outside the operation, such as an interrupt, only adds instructions.
				let count = (0..30)
					.map(|_| {
						let start = instructions();

						for _ in 0..batch {
							black_box(run());
						}

						instructions() - start
					})
					.min()
					.unwrap();

				format!("{} M", significant(count as f64 / f64::from(batch) / 1e6))
			}
		}
	}
}

/**
Two significant digits, as in `0.15`, `4.9`, `210`, and `1100`.
*/
fn significant(number: f64) -> String {
	let magnitude = number.log10().floor() as i32;
	let decimals = usize::try_from(1 - magnitude).unwrap_or(0);
	let scale = 10_f64.powi(magnitude - 1);
	format!("{:.decimals$}", (number / scale).round() * scale)
}

/**
The instructions that this process has run, which the kernel counts for each thread.
*/
#[cfg(target_os = "macos")]
fn instructions() -> u64 {
	unsafe extern "C" {
		fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut u64) -> i32;
	}

	// `RUSAGE_INFO_V4` fills a `rusage_info_v4` from `<sys/resource.h>`: a 16-byte UUID, and then 64-bit fields, of which `ri_instructions` is the 30th. The buffer is larger than the struct.
	let mut info = [0_u64; 64];
	let result =
		unsafe { proc_pid_rusage(std::process::id().try_into().unwrap(), 4, info.as_mut_ptr()) };
	assert_eq!(result, 0, "proc_pid_rusage failed");
	info[2 + 29]
}

#[cfg(not(target_os = "macos"))]
fn instructions() -> u64 {
	panic!("--instructions is only supported on macOS");
}

/**
A row of the table: the operation, and its cost with soml-lang, serde_json, and toml (or toml_edit for a document). An empty cell is an operation that the library does not have.
*/
type Row = (String, [Option<String>; 3]);

/**
The rows of a fixture that has a struct.
*/
fn struct_rows<X: Types>(fixture: &Fixture, meter: Meter) -> [Row; 2] {
	let data = fixture.data::<X>();

	[
		(
			format!("Read {} into a struct", fixture.name),
			[
				Some(meter.measure(|| soml::from_str::<X::Soml>(fixture.soml).unwrap())),
				Some(meter.measure(|| serde_json::from_str::<X::Json>(fixture.json).unwrap())),
				Some(meter.measure(|| toml::from_str::<X::Toml>(fixture.toml).unwrap())),
			],
		),
		(
			format!("Write {} from a struct", fixture.name),
			[
				Some(meter.measure(|| soml::to_string(&data.soml).unwrap())),
				Some(meter.measure(|| serde_json::to_string_pretty(&data.json).unwrap())),
				Some(meter.measure(|| toml::to_string(&data.toml).unwrap())),
			],
		),
	]
}

/**
The rows of every fixture: the value types, and the trees that keep comments and layout. serde_json has no such tree.
*/
fn value_rows(fixture: &Fixture, meter: Meter) -> [Row; 5] {
	let value: soml::Value = fixture.soml.parse().unwrap();
	let json_value: serde_json::Value = serde_json::from_str(fixture.json).unwrap();
	let toml_value: toml::Table = toml::from_str(fixture.toml).unwrap();
	let document: soml::Document = fixture.soml.parse().unwrap();
	let toml_document: toml_edit::DocumentMut = fixture.toml.parse().unwrap();

	[
		(
			format!("Read {} into a value type", fixture.name),
			[
				Some(meter.measure(|| fixture.soml.parse::<soml::Value>().unwrap())),
				Some(
					meter.measure(|| {
						serde_json::from_str::<serde_json::Value>(fixture.json).unwrap()
					}),
				),
				Some(meter.measure(|| toml::from_str::<toml::Table>(fixture.toml).unwrap())),
			],
		),
		(
			format!("Write {} from a value type", fixture.name),
			[
				Some(meter.measure(|| soml::to_string(&value).unwrap())),
				Some(meter.measure(|| serde_json::to_string_pretty(&json_value).unwrap())),
				Some(meter.measure(|| toml::to_string(&toml_value).unwrap())),
			],
		),
		(
			format!("Read {} into a document", fixture.name),
			[
				Some(meter.measure(|| fixture.soml.parse::<soml::Document>().unwrap())),
				None,
				Some(meter.measure(|| fixture.toml.parse::<toml_edit::DocumentMut>().unwrap())),
			],
		),
		(
			format!("Write {} from a document", fixture.name),
			[
				Some(meter.measure(|| document.to_string())),
				None,
				Some(meter.measure(|| toml_document.to_string())),
			],
		),
		(
			format!("Format {}", fixture.name),
			[
				Some(meter.measure(|| soml::format(fixture.soml).unwrap())),
				None,
				None,
			],
		),
	]
}

/**
Runs one operation many times, for a profiler or an instruction counter. The mode `none` does only the setup, so that its cost can be subtracted.
*/
fn run(fixture: &Fixture, mode: &str, iterations: usize) {
	let soml_text = fixture.soml;
	let value: soml::Value = soml_text.parse().unwrap();
	let document: soml::Document = soml_text.parse().unwrap();

	for _ in 0..iterations {
		match mode {
			"none" => {}
			"value" => drop(black_box(soml_text.parse::<soml::Value>().unwrap())),
			"value-serde" => drop(black_box(soml::from_str::<soml::Value>(soml_text).unwrap())),
			"write-value" => drop(black_box(soml::to_string(&value).unwrap())),
			"document" => drop(black_box(soml_text.parse::<soml::Document>().unwrap())),
			"document-write" => drop(black_box(document.to_string())),
			"format" => drop(black_box(soml::format(soml_text).unwrap())),
			_ => panic!("Unknown mode: {mode}"),
		}
	}
}

/**
Runs one operation on the fixture's struct many times: a soml-lang one, or `json-struct` or `json-write` for serde_json to compare with. The mode `none-struct` does only the setup.
*/
fn run_struct<X: Types>(fixture: &Fixture, mode: &str, iterations: usize) {
	let data = fixture.data::<X>();

	for _ in 0..iterations {
		match mode {
			"none-struct" => {}
			"struct" => drop(black_box(soml::from_str::<X::Soml>(fixture.soml).unwrap())),
			"write" => drop(black_box(soml::to_string(&data.soml).unwrap())),
			"canonical" => drop(black_box(soml::to_string_canonical(&data.soml).unwrap())),
			"json-struct" => drop(black_box(
				serde_json::from_str::<X::Json>(fixture.json).unwrap(),
			)),
			"json-write" => drop(black_box(serde_json::to_string_pretty(&data.json).unwrap())),
			_ => panic!("Unknown mode: {mode}"),
		}
	}
}

fn main() {
	let arguments: Vec<String> = std::env::args().skip(1).collect();

	let Some(mode) = arguments
		.first()
		.filter(|argument| *argument != "--instructions")
	else {
		let meter = if arguments.is_empty() {
			Meter::Time
		} else {
			Meter::Instructions
		};

		let mut rows = Vec::new();
		rows.extend(struct_rows::<ServiceTypes>(&SERVICE, meter));
		rows.extend(value_rows(&SERVICE, meter));
		rows.extend(value_rows(&PLATFORM, meter));
		rows.extend(struct_rows::<EarthquakesTypes>(&EARTHQUAKES, meter));
		rows.extend(value_rows(&EARTHQUAKES, meter));

		println!("| | soml-lang | serde_json | toml |");
		println!("|---|---|---|---|");

		for (operation, cells) in rows {
			let [soml, json, toml] = cells.map(Option::unwrap_or_default);
			println!("| {operation} | {soml} | {json} | {toml} |");
		}

		return;
	};

	let iterations = arguments.get(1).map_or(1, |text| {
		text.parse().expect("The iterations must be a number")
	});

	let name = std::env::var("FIXTURE").unwrap_or_else(|_| "earthquakes".to_owned());
	let fixture = FIXTURES
		.into_iter()
		.find(|fixture| fixture.name == name)
		.unwrap_or_else(|| panic!("Unknown fixture: {name}"));

	if !matches!(
		mode.as_str(),
		"none-struct" | "struct" | "write" | "canonical" | "json-struct" | "json-write"
	) {
		run(fixture, mode, iterations);
		return;
	}

	match fixture.name {
		"service" => run_struct::<ServiceTypes>(fixture, mode, iterations),
		"earthquakes" => run_struct::<EarthquakesTypes>(fixture, mode, iterations),
		name => panic!("The {name} fixture has no struct"),
	}
}
