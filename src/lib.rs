// Copyright 2018 The xi-editor Authors.
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
#![allow(
    clippy::identity_op,
    clippy::new_without_default,
    clippy::trivially_copy_pass_by_ref,
    deprecated
)]

use lazy_static::lazy_static;
use log::warn;
use std::collections::HashMap;
use std::string::ToString;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

#[cfg(feature = "chrome_trace_event")]
use std::fs;
#[cfg(feature = "chrome_trace_event")]
use std::path::Path;

mod categories;
mod config;
mod fixed_lifo_deque;
mod sample;
mod sys_pid;
mod sys_tid;

#[cfg(feature = "chrome_trace_event")]
pub mod chrome_trace_dump;

use crate::fixed_lifo_deque::FixedLifoDeque;

// Реэкспорт публичного API
pub use crate::categories::CategoriesT;
pub use crate::config::Config;
pub use crate::sample::{Sample, SampleArgs, SampleEventType, SampleGuard, StrCow, TracePayloadT};

fn exe_name() -> Option<String> {
    match std::env::current_exe() {
        Ok(exe_name) => match exe_name.file_name() {
            Some(filename) => filename.to_str().map(ToString::to_string),
            None => {
                let full_path = exe_name.into_os_string();
                let full_path_str = full_path.into_string();
                match full_path_str {
                    Ok(s) => Some(s),
                    Err(e) => {
                        warn!("Failed to get string representation: {:?}", e);
                        None
                    }
                }
            }
        },
        Err(ref e) => {
            warn!("Failed to get path to current exe: {:?}", e);
            None
        }
    }
}

pub struct Trace {
    enabled: AtomicBool,
    samples: Mutex<FixedLifoDeque<Sample>>,
}

impl Trace {
    pub fn disabled() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            samples: Mutex::new(FixedLifoDeque::new()),
        }
    }

    pub fn enabled(config: Config) -> Self {
        Self {
            enabled: AtomicBool::new(true),
            samples: Mutex::new(FixedLifoDeque::with_limit(config.max_samples())),
        }
    }

    pub fn disable(&self) {
        let mut all_samples = self.samples.lock().unwrap();
        all_samples.reset_limit(0);
        self.enabled.store(false, AtomicOrdering::Relaxed);
    }

    #[inline]
    pub fn enable(&self) {
        self.enable_config(Config::default());
    }

    pub fn enable_config(&self, config: Config) {
        let mut all_samples = self.samples.lock().unwrap();
        all_samples.reset_limit(config.max_samples());
        self.enabled.store(true, AtomicOrdering::Relaxed);
    }

    pub fn get_samples_count(&self) -> usize {
        self.samples.lock().unwrap().len()
    }

    pub fn get_samples_limit(&self) -> usize {
        self.samples.lock().unwrap().limit()
    }

    #[inline]
    pub(crate) fn record(&self, sample: Sample) {
        let mut all_samples = self.samples.lock().unwrap();
        all_samples.push_back(sample);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(AtomicOrdering::Relaxed)
    }

    pub fn instant<S, C>(&self, name: S, categories: C)
    where
        S: Into<StrCow>,
        C: Into<CategoriesT>,
    {
        if self.is_enabled() {
            self.record(Sample::new_instant(name, categories, None));
        }
    }

    pub fn instant_payload<S, C, P>(&self, name: S, categories: C, payload: P)
    where
        S: Into<StrCow>,
        C: Into<CategoriesT>,
        P: Into<TracePayloadT>,
    {
        if self.is_enabled() {
            self.record(Sample::new_instant(name, categories, Some(payload.into())));
        }
    }

    pub fn block<S, C>(&self, name: S, categories: C) -> SampleGuard<'_>
    where
        S: Into<StrCow>,
        C: Into<CategoriesT>,
    {
        if !self.is_enabled() {
            SampleGuard::new_disabled()
        } else {
            SampleGuard::new(self, name, categories, None)
        }
    }

    pub fn block_payload<S, C, P>(&self, name: S, categories: C, payload: P) -> SampleGuard<'_>
    where
        S: Into<StrCow>,
        C: Into<CategoriesT>,
        P: Into<TracePayloadT>,
    {
        if !self.is_enabled() {
            SampleGuard::new_disabled()
        } else {
            SampleGuard::new(self, name, categories, Some(payload.into()))
        }
    }

    pub fn closure<S, C, F, R>(&self, name: S, categories: C, closure: F) -> R
    where
        S: Into<StrCow>,
        C: Into<CategoriesT>,
        F: FnOnce() -> R,
    {
        let start = time::precise_time_ns();
        let result = closure();
        let end = time::precise_time_ns();
        if self.is_enabled() {
            self.record(Sample::new_duration(
                name,
                categories,
                None,
                start,
                end - start,
            ));
        }
        result
    }

    pub fn closure_payload<S, C, P, F, R>(
        &self,
        name: S,
        categories: C,
        closure: F,
        payload: P,
    ) -> R
    where
        S: Into<StrCow>,
        C: Into<CategoriesT>,
        P: Into<TracePayloadT>,
        F: FnOnce() -> R,
    {
        let start = time::precise_time_ns();
        let result = closure();
        let end = time::precise_time_ns();
        if self.is_enabled() {
            self.record(Sample::new_duration(
                name,
                categories,
                Some(payload.into()),
                start,
                end - start,
            ));
        }
        result
    }

    pub fn samples_cloned_unsorted(&self) -> Vec<Sample> {
        let all_samples = self.samples.lock().unwrap();
        if all_samples.is_empty() {
            return Vec::with_capacity(0);
        }

        let mut as_vec = Vec::with_capacity(all_samples.len() + 10);
        let first_sample_timestamp = all_samples.front().map_or(0, |s| s.timestamp_us);
        let tid = all_samples
            .front()
            .map_or_else(|| sys_tid::current_tid().unwrap(), |s| s.tid);

        if let Some(exe_name) = exe_name() {
            as_vec.push(Sample::new_metadata(
                first_sample_timestamp,
                sample::MetadataType::ProcessName { name: exe_name },
                tid,
            ));
        }

        let mut thread_names: HashMap<u64, StrCow> = HashMap::new();

        for sample in all_samples.iter() {
            if let Some(ref thread_name) = sample.thread_name {
                let previous_name = thread_names.insert(sample.tid, thread_name.clone());
                if previous_name.is_none() || previous_name.unwrap() != *thread_name {
                    as_vec.push(Sample::new_metadata(
                        first_sample_timestamp,
                        sample::MetadataType::ThreadName {
                            name: thread_name.to_string(),
                        },
                        sample.tid,
                    ));
                }
            }
        }

        as_vec.extend(all_samples.iter().cloned());
        as_vec
    }

    #[inline]
    pub fn samples_cloned_sorted(&self) -> Vec<Sample> {
        let mut samples = self.samples_cloned_unsorted();
        samples.sort_unstable();
        samples
    }

    #[cfg(feature = "chrome_trace_event")]
    pub fn save<P: AsRef<Path>>(
        &self,
        path: P,
        sort: bool,
    ) -> Result<(), chrome_trace_dump::Error> {
        let traces = if sort {
            samples_cloned_sorted()
        } else {
            samples_cloned_unsorted()
        };
        let path: &Path = path.as_ref();

        if path.exists() {
            return Err(chrome_trace_dump::Error::already_exists());
        }

        let mut trace_file = fs::File::create(path)?;

        chrome_trace_dump::serialize(&traces, &mut trace_file)
    }
}

lazy_static! {
    static ref TRACE: Trace = Trace::disabled();
}

#[inline]
pub fn enable_tracing() {
    TRACE.enable();
}

#[inline]
pub fn enable_tracing_with_config(config: Config) {
    TRACE.enable_config(config);
}

#[inline]
pub fn disable_tracing() {
    TRACE.disable();
}

#[inline]
pub fn is_enabled() -> bool {
    TRACE.is_enabled()
}

#[inline]
pub fn trace<S, C>(name: S, categories: C)
where
    S: Into<StrCow>,
    C: Into<CategoriesT>,
{
    TRACE.instant(name, categories);
}

#[inline]
pub fn trace_payload<S, C, P>(name: S, categories: C, payload: P)
where
    S: Into<StrCow>,
    C: Into<CategoriesT>,
    P: Into<TracePayloadT>,
{
    TRACE.instant_payload(name, categories, payload);
}

#[inline]
pub fn trace_block<'a, S, C>(name: S, categories: C) -> SampleGuard<'a>
where
    S: Into<StrCow>,
    C: Into<CategoriesT>,
{
    TRACE.block(name, categories)
}

#[inline]
pub fn trace_block_payload<'a, S, C, P>(name: S, categories: C, payload: P) -> SampleGuard<'a>
where
    S: Into<StrCow>,
    C: Into<CategoriesT>,
    P: Into<TracePayloadT>,
{
    TRACE.block_payload(name, categories, payload)
}

#[inline]
pub fn trace_closure<S, C, F, R>(name: S, categories: C, closure: F) -> R
where
    S: Into<StrCow>,
    C: Into<CategoriesT>,
    F: FnOnce() -> R,
{
    TRACE.closure(name, categories, closure)
}

#[inline]
pub fn trace_closure_payload<S, C, P, F, R>(name: S, categories: C, closure: F, payload: P) -> R
where
    S: Into<StrCow>,
    C: Into<CategoriesT>,
    P: Into<TracePayloadT>,
    F: FnOnce() -> R,
{
    TRACE.closure_payload(name, categories, closure, payload)
}

#[inline]
pub fn samples_len() -> usize {
    TRACE.get_samples_count()
}

#[inline]
pub fn samples_cloned_unsorted() -> Vec<Sample> {
    TRACE.samples_cloned_unsorted()
}

#[inline]
pub fn samples_cloned_sorted() -> Vec<Sample> {
    TRACE.samples_cloned_sorted()
}

#[cfg(feature = "chrome_trace_event")]
#[inline]
pub fn save<P: AsRef<Path>>(path: P, sort: bool) -> Result<(), chrome_trace_dump::Error> {
    TRACE.save(path, sort)
}

#[cfg(test)]
#[rustfmt::skip]
mod tests {
    use super::*;
    #[cfg(feature = "json_payload")]
    use serde_json::json;

    #[cfg(not(feature = "json_payload"))]
    fn to_payload(value: &'static str) -> &'static str {
        value
    }

    #[cfg(feature = "json_payload")]
    fn to_payload(value: &'static str) -> TracePayloadT {
        json!({"test": value})
    }

    #[test]
    fn test_samples_pulse() {
        let trace = Trace::enabled(Config::with_limit_count(10));
        for _i in 0..50 {
            trace.instant("test_samples_pulse", &["test"]);
        }
    }

    #[test]
    fn test_samples_block() {
        let trace = Trace::enabled(Config::with_limit_count(10));
        for _i in 0..50 {
            let _ = trace.block("test_samples_block", &["test"]);
        }
    }

    #[test]
    fn test_samples_closure() {
        let trace = Trace::enabled(Config::with_limit_count(10));
        for _i in 0..50 {
            trace.closure("test_samples_closure", &["test"], || {});
        }
    }

    #[test]
    fn test_disable_drops_all_samples() {
        let trace = Trace::enabled(Config::with_limit_count(10));
        assert!(trace.is_enabled());
        trace.instant("1", &["test"]);
        trace.instant("2", &["test"]);
        trace.instant("3", &["test"]);
        trace.instant("4", &["test"]);
        trace.instant("5", &["test"]);
        assert_eq!(trace.get_samples_count(), 5);
        assert_eq!(trace.samples_cloned_unsorted().len(), 7);
        trace.disable();
        assert_eq!(trace.get_samples_count(), 0);
    }

    #[test]
    fn test_get_samples() {
        let trace = Trace::enabled(Config::with_limit_count(20));
        assert_eq!(trace.samples_cloned_unsorted().len(), 0);

        assert!(trace.is_enabled());
        assert_eq!(trace.get_samples_limit(), 20);
        assert_eq!(trace.samples_cloned_unsorted().len(), 0);

        trace.closure_payload("x", &["test"], || (),
                              to_payload("test_get_samples"));
        assert_eq!(trace.get_samples_count(), 1);
        assert_eq!(trace.samples_cloned_unsorted().len(), 3);

        trace.closure_payload("y", &["test"], || {},
                              to_payload("test_get_samples"));
        assert_eq!(trace.samples_cloned_unsorted().len(), 4);

        trace.closure_payload("z", &["test"], || {},
                              to_payload("test_get_samples"));

        let snapshot = trace.samples_cloned_unsorted();
        assert_eq!(snapshot.len(), 5);

        assert_eq!(snapshot[0].name, "process_name");
        assert!(snapshot[0].args.as_ref().unwrap().metadata_name.as_ref().is_some());
        assert_eq!(snapshot[1].name, "thread_name");
        assert!(snapshot[1].args.as_ref().unwrap().metadata_name.as_ref().is_some());
        assert_eq!(snapshot[2].name, "x");
        assert_eq!(snapshot[3].name, "y");
        assert_eq!(snapshot[4].name, "z");
    }

    #[test]
    fn test_trace_disabled() {
        let trace = Trace::disabled();
        assert_eq!(trace.get_samples_limit(), 0);
        assert_eq!(trace.get_samples_count(), 0);

        {
            trace.instant("something", &[]);
            let _x = trace.block("something", &[]);
            trace.closure("something", &[], || ());
        }

        assert_eq!(trace.get_samples_count(), 0);
    }

    #[test]
    fn test_get_samples_nested_trace() {
        let trace = Trace::enabled(Config::with_limit_count(11));
        assert!(trace.is_enabled());
        assert_eq!(trace.get_samples_limit(), 11);

        trace.closure_payload("x", &["test"], || {
            trace.instant_payload("a", &["test"], to_payload("test_get_samples_nested_trace"));
            trace.closure_payload("y", &["test"], || {
                trace.instant_payload("b", &["test"], to_payload("test_get_samples_nested_trace"));
            }, to_payload("test_get_samples_nested_trace"));
            let _ = trace.block_payload("z", &["test"], to_payload("test_get_samples_nested_trace"));
            trace.instant_payload("c", &["test"], to_payload("test_get_samples_nested_trace"));
        }, to_payload("test_get_samples_nested_trace"));

        let snapshot = trace.samples_cloned_unsorted();
        assert_eq!(snapshot.len(), 9);

        assert_eq!(snapshot[0].name, "process_name");
        assert!(snapshot[0].args.as_ref().unwrap().metadata_name.as_ref().is_some());
        assert_eq!(snapshot[1].name, "thread_name");
        assert!(snapshot[1].args.as_ref().unwrap().metadata_name.as_ref().is_some());
        assert_eq!(snapshot[2].name, "a");
        assert_eq!(snapshot[3].name, "b");
        assert_eq!(snapshot[4].name, "y");
        assert_eq!(snapshot[5].name, "z");
        assert_eq!(snapshot[6].name, "z");
        assert_eq!(snapshot[7].name, "c");
        assert_eq!(snapshot[8].name, "x");
    }

    #[test]
    fn test_get_sorted_samples() {
        let trace = Trace::enabled(Config::with_limit_count(10));

        trace.closure_payload("x", &["test"], || {
            std::thread::sleep(std::time::Duration::new(0, 1000));
            trace.instant_payload("a", &["test"], to_payload("test_get_sorted_samples"));
            trace.closure_payload("y", &["test"], || {
                std::thread::sleep(std::time::Duration::new(0, 1000));
                trace.instant_payload("b", &["test"], to_payload("test_get_sorted_samples"));
            }, to_payload("test_get_sorted_samples"));
            let _ = trace.block_payload("z", &["test"], to_payload("test_get_sorted_samples"));
            trace.instant("c", &["test"]);
        }, to_payload("test_get_sorted_samples"));

        let snapshot = trace.samples_cloned_sorted();
        assert_eq!(snapshot.len(), 9);

        assert_eq!(snapshot[0].name, "process_name");
        assert!(snapshot[0].args.as_ref().unwrap().metadata_name.as_ref().is_some());
        assert_eq!(snapshot[1].name, "thread_name");
        assert!(snapshot[1].args.as_ref().unwrap().metadata_name.as_ref().is_some());
        assert_eq!(snapshot[2].name, "x");
        assert_eq!(snapshot[3].name, "a");
        assert_eq!(snapshot[4].name, "y");
        assert_eq!(snapshot[5].name, "b");
        assert_eq!(snapshot[6].name, "z");
        assert_eq!(snapshot[7].name, "z");
        assert_eq!(snapshot[8].name, "c");
    }
}
