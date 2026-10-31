// Copyright 2026 The LOC-OSS Community Authors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! API-compatibility tests for xi-trace.
//!
//! These tests lock in the *current* public API behaviour so that the
//! Rust 2018 → 2024 migration cannot silently regress it.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::thread;

#[cfg(feature = "chrome_trace_event")]
use xi_trace::chrome_trace_dump;
use xi_trace::{CategoriesT, Config, Sample, SampleEventType, SampleGuard, Trace, TracePayloadT};

static GLOBAL_LOCK: Mutex<()> = Mutex::new(());

fn hash<T: Hash>(t: &T) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

#[test]
fn config_with_limit_bytes() {
    let cfg = Config::with_limit_bytes(1024 * 1024);
    assert_eq!(
        cfg.max_samples(),
        1024 * 1024 / std::mem::size_of::<Sample>()
    );
}

#[test]
fn config_with_limit_count() {
    let cfg = Config::with_limit_count(42);
    assert_eq!(cfg.max_samples(), 42);
    assert_eq!(cfg.max_size_in_bytes(), 42 * std::mem::size_of::<Sample>());
}

#[test]
fn config_default_matches_one_mib_bytes_limit() {
    let d = Config::default();
    let explicit = Config::with_limit_bytes(1024 * 1024);
    assert_eq!(d.max_samples(), explicit.max_samples());
    assert_eq!(d.max_size_in_bytes(), explicit.max_size_in_bytes());
    assert!(d.max_size_in_bytes() <= 1024 * 1024);
}

#[test]
fn config_is_copy() {
    fn takes_copy(_c: Config) {}
    let cfg = Config::with_limit_count(1);
    takes_copy(cfg);
    takes_copy(cfg);
}

#[test]
fn categories_from_fixed_arrays_n1_through_n10() {
    fn join<C: Into<CategoriesT>>(c: C) -> String {
        c.into().join(",")
    }
    assert_eq!(join(&["a"]), "a");
    assert_eq!(join(&["a", "b"]), "a,b");
    assert_eq!(join(&["a", "b", "c"]), "a,b,c");
    assert_eq!(join(&["1", "2", "3", "4"]), "1,2,3,4");
    assert_eq!(join(&["1", "2", "3", "4", "5"]), "1,2,3,4,5");
    assert_eq!(join(&["1", "2", "3", "4", "5", "6"]), "1,2,3,4,5,6");
    assert_eq!(join(&["1", "2", "3", "4", "5", "6", "7"]), "1,2,3,4,5,6,7");
    assert_eq!(
        join(&["1", "2", "3", "4", "5", "6", "7", "8"]),
        "1,2,3,4,5,6,7,8"
    );
    assert_eq!(
        join(&["1", "2", "3", "4", "5", "6", "7", "8", "9"]),
        "1,2,3,4,5,6,7,8,9"
    );
    assert_eq!(
        join(&["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"]),
        "1,2,3,4,5,6,7,8,9,10"
    );
}

#[test]
fn categories_from_vec() {
    let c: CategoriesT = vec!["a".to_string(), "b".to_string()].into();
    assert_eq!(c.join(","), "a,b");
}

#[test]
fn categories_join() {
    let c: CategoriesT = (&["a", "b", "c"]).into();
    assert_eq!(c.join(", "), "a, b, c");
    assert_eq!(c.join(""), "abc");
    assert_eq!(c.join("::"), "a::b::c");
}

#[test]
fn categories_eq_cross_variant() {
    let s: CategoriesT = (&["a", "b"]).into();
    let d: CategoriesT = vec!["a".to_string(), "b".to_string()].into();
    assert_eq!(s, d);
    assert_eq!(d, s);

    let s_reversed: CategoriesT = (&["b", "a"]).into();
    assert_ne!(s, s_reversed);
    assert_ne!(s, (&["a"]).into());
    assert_ne!(s, (&["a", "b", "c"]).into());
}

#[test]
fn categories_serialize() {
    let c: CategoriesT = (&["a", "b"]).into();
    assert_eq!(serde_json::to_string(&c).unwrap(), r#""a,b""#);
}

#[test]
fn categories_deserialize_to_dynamic_array() {
    let c: CategoriesT = serde_json::from_str(r#""a,b,c""#).unwrap();
    assert_eq!(c.join(","), "a,b,c");
    match c {
        CategoriesT::DynamicArray(v) => assert_eq!(v, vec!["a", "b", "c"]),
        CategoriesT::StaticArray(_) => panic!("deserialize must yield DynamicArray"),
    }
}

#[test]
fn categories_deserialize_empty() {
    let c: CategoriesT = serde_json::from_str(r#""""#).unwrap();
    match c {
        CategoriesT::DynamicArray(v) => assert_eq!(v, vec![String::new()]),
        CategoriesT::StaticArray(_) => panic!("expected DynamicArray"),
    }
}

#[test]
fn sample_event_type_serialization_chars() {
    let cases = [
        (SampleEventType::DurationBegin, "B"),
        (SampleEventType::DurationEnd, "E"),
        (SampleEventType::CompleteDuration, "X"),
        (SampleEventType::Instant, "i"),
        (SampleEventType::AsyncStart, "b"),
        (SampleEventType::AsyncInstant, "n"),
        (SampleEventType::AsyncEnd, "e"),
        (SampleEventType::FlowStart, "s"),
        (SampleEventType::FlowInstant, "t"),
        (SampleEventType::FlowEnd, "f"),
        (SampleEventType::ObjectCreated, "N"),
        (SampleEventType::ObjectSnapshot, "O"),
        (SampleEventType::ObjectDestroyed, "D"),
        (SampleEventType::Metadata, "M"),
    ];
    for (variant, expected) in cases {
        let mut s = Sample::new_instant("probe", &[], None);
        s.event_type = variant;
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["ph"], expected, "variant {:?}", variant);

        let back: Sample = serde_json::from_value(v).unwrap();
        assert_eq!(back.event_type, variant);
    }
}

#[test]
fn sample_new_instant_basics() {
    let s = Sample::new_instant("evt", &["cat"], None);
    assert_eq!(s.name, "evt");
    assert!(matches!(s.categories, Some(CategoriesT::StaticArray(_))));
    assert_eq!(s.event_type, SampleEventType::Instant);
    assert!(s.duration_us.is_none());
    assert!(s.pid > 0);
    assert!(s.tid > 0);
    assert!(s.args.is_some());
    assert!(s.args.as_ref().unwrap().payload.is_none());
}

#[cfg(not(feature = "json_payload"))]
#[test]
fn sample_new_instant_with_str_payload() {
    let s = Sample::new_instant("evt", &["cat"], Some("payload".into()));
    assert_eq!(s.args.as_ref().unwrap().payload.as_deref(), Some("payload"));
}

#[cfg(feature = "json_payload")]
#[test]
fn sample_new_instant_with_json_payload() {
    let value = serde_json::json!({ "a": 1 });
    let s = Sample::new_instant("evt", &["cat"], Some(value.clone()));
    assert_eq!(s.args.as_ref().unwrap().payload, Some(value));
}

#[test]
fn sample_new_duration_ns_to_us_conversion() {
    let s = Sample::new_duration("evt", &["cat"], None, 1_000, 5_000);
    assert_eq!(s.event_type, SampleEventType::CompleteDuration);
    assert_eq!(s.timestamp_us, 1);
    assert_eq!(s.duration_us, Some(5));
}

#[test]
fn sample_new_duration_marker() {
    let s = Sample::new_duration_marker("evt", &["cat"], None, SampleEventType::DurationBegin);
    assert_eq!(s.event_type, SampleEventType::DurationBegin);
    assert!(s.duration_us.is_none());
}

#[test]
fn sample_partial_eq_ignores_duration_and_thread_name() {
    let a = Sample::new_duration("evt", &["cat"], None, 0, 1_000);
    let b = Sample::new_duration("evt", &["cat"], None, 0, 2_000);
    assert_ne!(a.duration_us, b.duration_us);
    assert_eq!(a, b);
}

#[test]
fn sample_ord_by_timestamp() {
    let mut a = Sample::new_instant("a", &[], None);
    let mut b = Sample::new_instant("b", &[], None);
    a.timestamp_us = 10;
    b.timestamp_us = 5;
    assert!(a > b);

    let mut v = [a, b];
    v.sort();
    assert_eq!(v[0].timestamp_us, 5);
    assert_eq!(v[1].timestamp_us, 10);
}

#[test]
fn sample_hash_is_stable_for_same_pid_and_timestamp() {
    let mut a = Sample::new_instant("a", &[], None);
    let mut b = Sample::new_instant("b", &[], None);
    a.pid = 1;
    a.timestamp_us = 100;
    b.pid = 1;
    b.timestamp_us = 100;

    assert_eq!(hash(&a), hash(&b));
    assert_ne!(a, b);
}

#[test]
fn sample_hashset_dedups_equal_samples() {
    let mut a = Sample::new_instant("same", &[], None);
    let mut b = Sample::new_instant("same", &[], None);
    a.pid = 1;
    a.timestamp_us = 100;
    b.pid = 1;
    b.timestamp_us = 100;

    assert_eq!(a, b);
    assert_eq!(hash(&a), hash(&b));

    let mut set = HashSet::new();
    set.insert(a);
    set.insert(b);
    assert_eq!(set.len(), 1);
}

#[test]
fn sample_serde_roundtrip() {
    let mut s = Sample::new_instant("evt", &["cat", "other"], None);
    s.pid = 7;
    s.tid = 8;
    s.timestamp_us = 42;

    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains(r#""ph":"i""#));
    assert!(json.contains(r#""cat":"cat,other""#));

    let back: Sample = serde_json::from_str(&json).unwrap();
    assert_eq!(s, back);
    assert_eq!(back.tid, 8);
    assert_eq!(back.pid, 7);
}

#[test]
fn sample_guard_disabled_is_noop() {
    let _g = SampleGuard::new_disabled();
}

#[test]
fn trace_disabled_records_nothing() {
    let t = Trace::disabled();
    assert!(!t.is_enabled());
    t.instant("x", &[]);
    let _g = t.block("x", &[]);
    let _: () = t.closure("x", &[], || ());
    assert_eq!(t.get_samples_count(), 0);
}

#[test]
fn trace_enable_disable_clears_samples() {
    let t = Trace::enabled(Config::with_limit_count(10));
    t.instant("a", &[]);
    t.instant("b", &[]);
    assert_eq!(t.get_samples_count(), 2);
    t.disable();
    assert!(!t.is_enabled());
    assert_eq!(t.get_samples_count(), 0);
    assert_eq!(t.get_samples_limit(), 0);

    t.enable();
    assert!(t.is_enabled());
    assert_eq!(t.get_samples_limit(), Config::default().max_samples());

    t.enable_config(Config::with_limit_count(5));
    assert_eq!(t.get_samples_limit(), 5);
}

#[test]
fn trace_closure_returns_closure_value() {
    let t = Trace::enabled(Config::with_limit_count(10));
    let r = t.closure("x", &[], || 42);
    assert_eq!(r, 42);
    assert_eq!(t.get_samples_count(), 1);
}

#[cfg(not(feature = "json_payload"))]
fn payload_arg() -> TracePayloadT {
    "payload".into()
}

#[cfg(feature = "json_payload")]
fn payload_arg() -> TracePayloadT {
    serde_json::json!({ "k": "v" })
}

#[test]
fn trace_closure_payload_returns_value() {
    let t = Trace::enabled(Config::with_limit_count(10));
    let r = t.closure_payload("x", &[], || 7, payload_arg());
    assert_eq!(r, 7);
    assert_eq!(t.get_samples_count(), 1);
}

#[test]
fn trace_samples_cloned_unsorted_contains_metadata() {
    let t = Trace::enabled(Config::with_limit_count(20));
    t.instant("my-event", &[]);

    let samples = t.samples_cloned_unsorted();
    assert!(samples.iter().any(|s| s.name == "process_name"));
    assert!(samples.iter().any(|s| s.name == "my-event"));
    let actual: Vec<_> = samples
        .iter()
        .filter(|s| s.event_type != SampleEventType::Metadata)
        .collect();
    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].name, "my-event");
}

#[test]
fn trace_samples_cloned_sorted_is_monotonic() {
    let t = Trace::enabled(Config::with_limit_count(20));
    t.closure("outer", &[], || {
        std::thread::sleep(std::time::Duration::from_micros(2));
        t.instant("inner", &[]);
    });

    let samples = t.samples_cloned_sorted();
    let mut prev = 0u64;
    for s in &samples {
        assert!(
            s.timestamp_us >= prev,
            "samples not monotonic: {} < {}",
            s.timestamp_us,
            prev
        );
        prev = s.timestamp_us;
    }
}

#[test]
fn trace_sample_limit_enforced() {
    let t = Trace::enabled(Config::with_limit_count(3));
    for i in 0..10 {
        t.instant(format!("e{}", i), &[]);
    }
    assert_eq!(t.get_samples_count(), 3);
}

#[test]
fn trace_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Trace>();
    assert_send_sync::<Arc<Trace>>();
    assert_send_sync::<Sample>();
}

#[test]
fn trace_thread_safe() {
    let t = Arc::new(Trace::enabled(Config::with_limit_count(10_000)));
    let mut handles = Vec::new();
    for tid in 0..4 {
        let tc = t.clone();
        handles.push(thread::spawn(move || {
            for i in 0..100 {
                tc.instant(format!("t{}-{}", tid, i), &[]);
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert!(t.get_samples_count() > 0);
    let _ = t.samples_cloned_unsorted();
}
#[cfg(feature = "chrome_trace_event")]
#[test]
fn chrome_trace_serialize_empty() {
    let samples: Vec<Sample> = Vec::new();
    let mut buf = Vec::new();
    chrome_trace_dump::serialize(&samples, &mut buf).unwrap();
    assert_eq!(buf, b"[]");
}
#[cfg(feature = "chrome_trace_event")]
#[test]
fn chrome_trace_to_value_decode_roundtrip() {
    let mut s = Sample::new_instant("x", &["cat"], None);
    s.pid = 1;
    s.tid = 2;
    s.timestamp_us = 42;

    let samples = vec![s];
    let v = chrome_trace_dump::to_value(&samples).unwrap();
    let back = chrome_trace_dump::decode(v).unwrap();
    assert_eq!(samples, back);
}
#[cfg(feature = "chrome_trace_event")]
#[test]
fn chrome_trace_error_already_exists() {
    let err = chrome_trace_dump::Error::already_exists();
    match err {
        chrome_trace_dump::Error::Io(e) => {
            assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists)
        }
        other => panic!("expected Io(AlreadyExists), got {:?}", other),
    }
}
#[cfg(feature = "chrome_trace_event")]
#[test]
fn chrome_trace_error_from_io() {
    let io_err = std::io::Error::other("boom");
    let e: chrome_trace_dump::Error = io_err.into();
    assert!(matches!(e, chrome_trace_dump::Error::Io(_)));
}
#[cfg(feature = "chrome_trace_event")]
#[test]
fn chrome_trace_error_from_json() {
    let json_err = serde_json::from_str::<i32>("not-json").unwrap_err();
    let e: chrome_trace_dump::Error = json_err.into();
    assert!(matches!(e, chrome_trace_dump::Error::Json(_)));
}
#[cfg(feature = "chrome_trace_event")]
#[test]
fn chrome_trace_error_from_string() {
    let e: chrome_trace_dump::Error = "custom".to_string().into();
    assert!(matches!(e, chrome_trace_dump::Error::DecodingFormat(_)));
}

#[cfg(feature = "chrome_trace_event")]
#[test]
fn trace_save_creates_file_and_rejects_existing() {
    let t = Trace::enabled(Config::with_limit_count(10));
    t.instant("x", &[]);

    let path = std::env::temp_dir().join(format!(
        "xi_trace_compat_{}_{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    t.save(&path, true).unwrap();
    assert!(path.exists());

    let err = t.save(&path, true).unwrap_err();
    match err {
        chrome_trace_dump::Error::Io(e) => {
            assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists)
        }
        other => panic!("expected Io(AlreadyExists), got {:?}", other),
    }

    std::fs::remove_file(&path).unwrap();
}

#[test]
fn global_api_smoke() {
    let _guard = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    xi_trace::disable_tracing();
    assert!(!xi_trace::is_enabled());
    assert_eq!(xi_trace::samples_len(), 0);

    xi_trace::enable_tracing_with_config(Config::with_limit_count(10));
    assert!(xi_trace::is_enabled());

    xi_trace::trace("hello", &["test"]);
    assert_eq!(xi_trace::samples_len(), 1);

    xi_trace::disable_tracing();
    assert_eq!(xi_trace::samples_len(), 0);
}

#[cfg(feature = "json_payload")]
#[test]
fn json_payload_roundtrip() {
    let t = Trace::enabled(Config::with_limit_count(10));
    let value = serde_json::json!({ "a": 1, "b": [true, null] });
    t.instant_payload("evt", &[], value.clone());

    let samples = t.samples_cloned_unsorted();
    let found = samples
        .iter()
        .any(|s| s.args.as_ref().and_then(|a| a.payload.as_ref()) == Some(&value));
    assert!(found);
}

#[test]
fn config_with_limit_bytes_zero() {
    let cfg = Config::with_limit_bytes(0);
    assert_eq!(cfg.max_samples(), 0);
    assert_eq!(cfg.max_size_in_bytes(), 0);
}

#[test]
fn config_with_limit_bytes_less_than_sample_size() {
    let sz = std::mem::size_of::<Sample>();
    let cfg = Config::with_limit_bytes(sz - 1);
    assert_eq!(cfg.max_samples(), 0);
    assert_eq!(cfg.max_size_in_bytes(), 0);
}

#[test]
fn categories_from_empty_static_array() {
    let empty: &'static [&'static str; 0] = &[];
    let c: CategoriesT = empty.into();
    match c {
        CategoriesT::StaticArray(arr) => assert!(arr.is_empty()),
        CategoriesT::DynamicArray(_) => panic!("expected StaticArray"),
    }
}

#[test]
fn categories_serialize_empty_static_array() {
    let empty: &'static [&'static str; 0] = &[];
    let c: CategoriesT = empty.into();
    assert_eq!(serde_json::to_string(&c).unwrap(), r#""""#);
}

#[test]
fn categories_deserialize_single_no_comma() {
    let c: CategoriesT = serde_json::from_str(r#""solo""#).unwrap();
    match c {
        CategoriesT::DynamicArray(v) => assert_eq!(v, vec!["solo".to_string()]),
        CategoriesT::StaticArray(_) => panic!("expected DynamicArray"),
    }
}

#[test]
fn categories_eq_empty_static_vs_empty_dynamic() {
    let empty: &'static [&'static str; 0] = &[];
    let s: CategoriesT = empty.into();
    let d: CategoriesT = Vec::<String>::new().into();
    assert_eq!(s, d);
    assert_eq!(d, s);
}

#[test]
fn categories_join_empty_separator() {
    let c: CategoriesT = (&["a", "b", "c"]).into();
    assert_eq!(c.join(""), "abc");
}

#[test]
fn sample_new_duration_zero() {
    let s = Sample::new_duration("e", &[], None, 1_000, 0);
    assert_eq!(s.timestamp_us, 1);
    assert_eq!(s.duration_us, Some(0));
    assert_eq!(s.event_type, SampleEventType::CompleteDuration);
}

#[test]
fn sample_new_duration_sub_microsecond() {
    let s = Sample::new_duration("e", &[], None, 500, 999);
    assert_eq!(s.timestamp_us, 0);
    assert_eq!(s.duration_us, Some(0));
}

#[test]
fn sample_new_duration_marker_end() {
    let s = Sample::new_duration_marker("e", &["c"], None, SampleEventType::DurationEnd);
    assert_eq!(s.event_type, SampleEventType::DurationEnd);
    assert!(s.duration_us.is_none());
    assert_eq!(s.name, "e");
}

#[test]
fn sample_new_instant_empty_categories() {
    let empty: &'static [&'static str; 0] = &[];
    let s = Sample::new_instant("x", empty, None);
    match s.categories {
        Some(CategoriesT::StaticArray(a)) => assert!(a.is_empty()),
        _ => panic!("expected Some(StaticArray)"),
    }
}

#[test]
fn sample_eq_differs_on_pid() {
    let mut a = Sample::new_instant("x", &[], None);
    let mut b = Sample::new_instant("x", &[], None);
    a.pid = 1;
    a.tid = 1;
    a.timestamp_us = 100;
    b.pid = 2;
    b.tid = 1;
    b.timestamp_us = 100;
    assert_ne!(a, b);
}

#[test]
fn sample_eq_differs_on_tid() {
    let mut a = Sample::new_instant("x", &[], None);
    let mut b = Sample::new_instant("x", &[], None);
    a.pid = 1;
    a.tid = 1;
    a.timestamp_us = 100;
    b.pid = 1;
    b.tid = 2;
    b.timestamp_us = 100;
    assert_ne!(a, b);
}

#[test]
fn sample_eq_differs_on_event_type() {
    let mut a = Sample::new_instant("x", &[], None);
    let mut b = Sample::new_instant("x", &[], None);
    a.pid = 1;
    a.tid = 1;
    a.timestamp_us = 100;
    b.pid = 1;
    b.tid = 1;
    b.timestamp_us = 100;
    b.event_type = SampleEventType::DurationBegin;
    assert_ne!(a, b);
}

#[test]
fn sample_eq_ignores_thread_name() {
    let mut a = Sample::new_instant("x", &[], None);
    let mut b = Sample::new_instant("x", &[], None);
    a.pid = 1;
    a.tid = 1;
    a.timestamp_us = 100;
    b.pid = 1;
    b.tid = 1;
    b.timestamp_us = 100;
    a.thread_name = Some("foo".into());
    b.thread_name = Some("bar".into());
    assert_eq!(a, b);
}

#[test]
fn sample_eq_differs_on_payload() {
    let mut a = Sample::new_instant("x", &[], None);
    let mut b = Sample::new_instant("x", &[], Some(payload_arg()));
    a.pid = 1;
    a.tid = 1;
    a.timestamp_us = 100;
    b.pid = 1;
    b.tid = 1;
    b.timestamp_us = 100;
    assert_ne!(a, b);
}

#[test]
fn sample_partial_cmp_returns_some() {
    let a = Sample::new_instant("a", &[], None);
    let b = Sample::new_instant("b", &[], None);
    assert!(a.partial_cmp(&b).is_some());
}

#[test]
fn sample_guard_drop_records_end_event() {
    let t = Trace::enabled(Config::with_limit_count(10));
    {
        let _g = t.block("blk", &["c"]);
        assert_eq!(t.get_samples_count(), 1);
    }
    assert_eq!(t.get_samples_count(), 2);
    let s = t.samples_cloned_unsorted();
    let blk: Vec<_> = s.iter().filter(|x| x.name == "blk").collect();
    assert_eq!(blk.len(), 2);
    assert_eq!(blk[0].event_type, SampleEventType::DurationBegin);
    assert_eq!(blk[1].event_type, SampleEventType::DurationEnd);
}

#[test]
fn trace_disabled_samples_cloned_empty() {
    let t = Trace::disabled();
    assert!(t.samples_cloned_unsorted().is_empty());
    assert!(t.samples_cloned_sorted().is_empty());
}

#[test]
fn trace_enabled_zero_limit() {
    let t = Trace::enabled(Config::with_limit_count(0));
    assert!(t.is_enabled());
    t.instant("x", &[]);
    assert_eq!(t.get_samples_count(), 0);
    assert_eq!(t.get_samples_limit(), 0);
}

#[test]
fn trace_block_payload_disabled_is_noop() {
    let t = Trace::disabled();
    let _g = t.block_payload("x", &[], payload_arg());
    assert_eq!(t.get_samples_count(), 0);
}

#[test]
fn trace_closure_payload_disabled_returns_value() {
    let t = Trace::disabled();
    let r = t.closure_payload("x", &[], || 42, payload_arg());
    assert_eq!(r, 42);
    assert_eq!(t.get_samples_count(), 0);
}

#[test]
fn trace_named_thread_adds_thread_name_metadata() {
    let t = Arc::new(Trace::enabled(Config::with_limit_count(100)));
    let tc = t.clone();
    let h = thread::Builder::new()
        .name("compat-thread".to_string())
        .spawn(move || {
            tc.instant("inside", &[]);
        })
        .unwrap();
    h.join().unwrap();

    let samples = t.samples_cloned_unsorted();
    let has_thread_name = samples.iter().any(|s| {
        s.name == "thread_name"
            && s.args
                .as_ref()
                .and_then(|a| a.metadata_name.as_ref())
                .map(|n| n.as_ref() == "compat-thread")
                .unwrap_or(false)
    });
    assert!(has_thread_name);
}

#[cfg(feature = "chrome_trace_event")]
#[test]
fn trace_save_unsorted_works() {
    let t = Trace::enabled(Config::with_limit_count(10));
    t.instant("x", &[]);

    let path = std::env::temp_dir().join(format!(
        "xi_trace_compat_unsorted_{}_{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    t.save(&path, false).unwrap();
    assert!(path.exists());
    std::fs::remove_file(&path).unwrap();
}
#[cfg(feature = "chrome_trace_event")]
#[test]
fn chrome_trace_deserialize_from_reader() {
    let mut s = Sample::new_instant("x", &["c"], None);
    s.pid = 1;
    s.tid = 2;
    s.timestamp_us = 42;
    let samples = vec![s];

    let json = serde_json::to_string(&samples).unwrap();
    let back = chrome_trace_dump::deserialize(json.as_bytes()).unwrap();
    assert_eq!(samples, back);
}
#[cfg(feature = "chrome_trace_event")]
#[test]
fn chrome_trace_to_value_decode_empty() {
    let samples: Vec<Sample> = Vec::new();
    let v = chrome_trace_dump::to_value(&samples).unwrap();
    assert_eq!(v, serde_json::json!([]));
    let back = chrome_trace_dump::decode(v).unwrap();
    assert!(back.is_empty());
}
#[cfg(feature = "chrome_trace_event")]
#[test]
fn chrome_trace_serialize_non_empty() {
    let mut s = Sample::new_instant("x", &["c"], None);
    s.pid = 1;
    s.tid = 2;
    s.timestamp_us = 42;
    let samples = vec![s];

    let mut buf = Vec::new();
    chrome_trace_dump::serialize(&samples, &mut buf).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&buf).unwrap();
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "x");
    assert_eq!(arr[0]["ph"], "i");
}
#[cfg(feature = "chrome_trace_event")]
#[test]
fn chrome_trace_deserialize_invalid_json_is_error() {
    let r = chrome_trace_dump::deserialize(&b"not json"[..]);
    assert!(matches!(r, Err(chrome_trace_dump::Error::Json(_))));
}

#[test]
fn global_enable_tracing_uses_default() {
    let _g = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    xi_trace::disable_tracing();
    xi_trace::enable_tracing();
    assert!(xi_trace::is_enabled());
    assert_eq!(xi_trace::samples_len(), 0);
    xi_trace::disable_tracing();
}

#[test]
fn global_trace_payload_works() {
    let _g = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    xi_trace::disable_tracing();
    xi_trace::enable_tracing_with_config(Config::with_limit_count(10));
    xi_trace::trace_payload("evt", &[], payload_arg());
    assert_eq!(xi_trace::samples_len(), 1);
    xi_trace::disable_tracing();
}

#[test]
fn global_trace_block_and_closure_work() {
    let _g = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    xi_trace::disable_tracing();
    xi_trace::enable_tracing_with_config(Config::with_limit_count(10));

    {
        let _b = xi_trace::trace_block("b", &[]);
    }
    let r = xi_trace::trace_closure("c", &[], || 7);
    assert_eq!(r, 7);

    assert_eq!(xi_trace::samples_len(), 3);

    let unsorted = xi_trace::samples_cloned_unsorted();
    let sorted = xi_trace::samples_cloned_sorted();
    assert!(!unsorted.is_empty());
    assert!(!sorted.is_empty());

    xi_trace::disable_tracing();
}
