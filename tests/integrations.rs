/*!
The `jiff` and `chrono` features: conversions at the edges of each range, and the modules for `#[serde(with)]`.
*/

#[cfg(feature = "jiff")]
mod jiff_feature {
	use jiff::{SignedDuration, Timestamp};
	use soml::{Duration, Instant};

	#[test]
	fn timestamps_convert_within_both_ranges() {
		assert_eq!(
			Instant::try_from(Timestamp::new(1_789_826_400, 500).expect("valid"))
				.expect("in range")
				.to_string(),
			"2026-09-19T14:00:00.0000005Z"
		);
		assert_eq!(
			Timestamp::try_from(Instant::MIN)
				.expect("in range")
				.to_string(),
			"0001-01-01T00:00:00Z"
		);
		assert_eq!(
			Instant::try_from(Timestamp::MAX)
				.expect("in range")
				.to_string(),
			"9999-12-30T22:00:00.999999999Z"
		);

		// jiff's range ends about a day before SOML's, and starts long before it.
		assert!(Timestamp::try_from(Instant::MAX).is_err());
		assert!(Instant::try_from(Timestamp::MIN).is_err());
	}

	#[test]
	fn durations_convert_within_both_ranges() {
		for duration in [
			Duration::MIN,
			Duration::ZERO,
			Duration::MAX,
			Duration::from_nanoseconds(-1_500),
		] {
			assert_eq!(
				Duration::try_from(SignedDuration::from(duration)).expect("in range"),
				duration
			);
		}

		assert!(Duration::try_from(SignedDuration::MAX).is_err());
		assert!(Duration::try_from(SignedDuration::MIN).is_err());
	}

	#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
	struct Deploy {
		#[serde(with = "soml::jiff::timestamp")]
		at: Timestamp,
		#[serde(with = "soml::jiff::signed_duration")]
		took: SignedDuration,
		#[serde(with = "soml::jiff::timestamp::option", default)]
		finished: Option<Timestamp>,
	}

	#[test]
	fn the_with_modules_write_native_values_and_read_them_back() {
		let text = "at: 2026-09-19T14:00:00Z\ntook: -1.5s\nfinished: 2026-09-19T15:00:00Z\n";
		let deploy: Deploy = soml::from_str(text).expect("valid");
		assert_eq!(deploy.took, SignedDuration::from_millis(-1500));
		assert_eq!(soml::to_string(&deploy).expect("written"), text);

		let deploy: Deploy = soml::from_str("at: 2026-09-19T14:00:00Z\ntook: 0s").expect("valid");
		assert_eq!(deploy.finished, None);
	}

	#[test]
	fn the_with_modules_read_only_native_values() {
		assert!(soml::from_str::<Deploy>("at: '2026-09-19T14:00:00Z'\ntook: 0s").is_err());
		assert!(soml::from_str::<Deploy>("at: 9999-12-31T00:00:00Z\ntook: 0s").is_err());
	}

	#[test]
	fn the_with_modules_refuse_to_write_a_value_outside_the_soml_range() {
		// Each case has one value outside the range, so each `with` module is checked on its own.
		let deploy = Deploy {
			at: Timestamp::UNIX_EPOCH,
			took: SignedDuration::MAX,
			finished: None,
		};
		assert!(soml::to_string(&deploy).is_err());

		let deploy = Deploy {
			took: SignedDuration::ZERO,
			finished: Some(Timestamp::MIN),
			..deploy
		};
		assert!(soml::to_string(&deploy).is_err());

		let deploy = Deploy {
			finished: None,
			..deploy
		};
		assert!(soml::to_string(&deploy).is_ok());
	}
}

#[cfg(feature = "chrono")]
mod chrono_feature {
	use chrono::{DateTime, TimeDelta, Utc};
	use soml::{Duration, Instant};

	#[test]
	fn date_times_convert_within_both_ranges() {
		for instant in [
			Instant::MIN,
			Instant::MAX,
			Instant::from_unix(-1, 999_999_999).expect("valid"),
		] {
			assert_eq!(
				Instant::try_from(DateTime::<Utc>::from(instant)).expect("in range"),
				instant
			);
		}

		assert!(Instant::try_from(DateTime::<Utc>::MAX_UTC).is_err());
		assert!(Instant::try_from(DateTime::<Utc>::MIN_UTC).is_err());
	}

	#[test]
	fn durations_convert_within_both_ranges() {
		for duration in [
			Duration::MIN,
			Duration::ZERO,
			Duration::MAX,
			Duration::from_nanoseconds(-1_500),
		] {
			assert_eq!(
				Duration::try_from(TimeDelta::from(duration)).expect("in range"),
				duration
			);
		}

		assert!(Duration::try_from(TimeDelta::MAX).is_err());
		assert!(Duration::try_from(TimeDelta::MIN).is_err());
	}

	#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
	struct Deploy {
		#[serde(with = "soml::chrono::date_time")]
		at: DateTime<Utc>,
		#[serde(with = "soml::chrono::time_delta::option")]
		took: Option<TimeDelta>,
	}

	#[test]
	fn the_with_modules_write_native_values_and_read_them_back() {
		let text = "at: 2026-09-19T14:00:00.5Z\ntook: 1h30m\n";
		let deploy: Deploy = soml::from_str(text).expect("valid");
		assert_eq!(deploy.took, Some(TimeDelta::minutes(90)));
		assert_eq!(soml::to_string(&deploy).expect("written"), text);

		let deploy: Deploy = soml::from_str("at: 2026-09-19T14:00:00Z\ntook: null").expect("valid");
		assert_eq!(deploy.took, None);
		assert!(soml::from_str::<Deploy>("at: '2026-09-19T14:00:00Z'\ntook: null").is_err());
	}

	#[test]
	fn the_with_modules_refuse_to_write_a_value_outside_the_soml_range() {
		let deploy = Deploy {
			at: DateTime::<Utc>::MAX_UTC,
			took: None,
		};
		assert!(soml::to_string(&deploy).is_err());

		let deploy = Deploy {
			at: DateTime::<Utc>::UNIX_EPOCH,
			took: Some(TimeDelta::MAX),
		};
		assert!(soml::to_string(&deploy).is_err());
	}
}

#[cfg(feature = "chrono")]
#[test]
fn a_chrono_leap_second_is_an_error_that_says_so() {
	use chrono::{NaiveDate, Utc};

	let leap = NaiveDate::from_ymd_opt(2016, 12, 31)
		.and_then(|date| date.and_hms_nano_opt(23, 59, 59, 1_500_000_000))
		.expect("a leap second")
		.and_utc();
	let error = soml::Instant::try_from(leap.with_timezone(&Utc)).unwrap_err();
	assert!(error.message().contains("leap second"), "{error}");
}
