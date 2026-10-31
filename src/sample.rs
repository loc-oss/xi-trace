use std::borrow::Cow;
use std::cmp;
use std::hash::{Hash, Hasher};

use serde_derive::{Deserialize, Serialize};

use crate::Trace;
use crate::categories::CategoriesT;
use crate::{sys_pid, sys_tid};

pub type StrCow = Cow<'static, str>;

#[cfg(not(feature = "json_payload"))]
pub type TracePayloadT = StrCow;

#[cfg(feature = "json_payload")]
pub type TracePayloadT = serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleEventType {
    DurationBegin,
    DurationEnd,
    CompleteDuration,
    Instant,
    AsyncStart,
    AsyncInstant,
    AsyncEnd,
    FlowStart,
    FlowInstant,
    FlowEnd,
    ObjectCreated,
    ObjectSnapshot,
    ObjectDestroyed,
    Metadata,
}

impl SampleEventType {
    #[inline]
    fn into_chrome_id(self) -> char {
        match self {
            SampleEventType::DurationBegin => 'B',
            SampleEventType::DurationEnd => 'E',
            SampleEventType::CompleteDuration => 'X',
            SampleEventType::Instant => 'i',
            SampleEventType::AsyncStart => 'b',
            SampleEventType::AsyncInstant => 'n',
            SampleEventType::AsyncEnd => 'e',
            SampleEventType::FlowStart => 's',
            SampleEventType::FlowInstant => 't',
            SampleEventType::FlowEnd => 'f',
            SampleEventType::ObjectCreated => 'N',
            SampleEventType::ObjectSnapshot => 'O',
            SampleEventType::ObjectDestroyed => 'D',
            SampleEventType::Metadata => 'M',
        }
    }

    #[inline]
    fn from_chrome_id(symbol: char) -> Self {
        match symbol {
            'B' => SampleEventType::DurationBegin,
            'E' => SampleEventType::DurationEnd,
            'X' => SampleEventType::CompleteDuration,
            'i' => SampleEventType::Instant,
            'b' => SampleEventType::AsyncStart,
            'n' => SampleEventType::AsyncInstant,
            'e' => SampleEventType::AsyncEnd,
            's' => SampleEventType::FlowStart,
            't' => SampleEventType::FlowInstant,
            'f' => SampleEventType::FlowEnd,
            'N' => SampleEventType::ObjectCreated,
            'O' => SampleEventType::ObjectSnapshot,
            'D' => SampleEventType::ObjectDestroyed,
            'M' => SampleEventType::Metadata,
            _ => panic!("Unexpected chrome sample type '{}'", symbol),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MetadataType {
    ProcessName {
        name: String,
    },
    #[allow(dead_code)]
    ProcessLabels {
        labels: String,
    },
    #[allow(dead_code)]
    ProcessSortIndex {
        sort_index: i32,
    },
    ThreadName {
        name: String,
    },
    #[allow(dead_code)]
    ThreadSortIndex {
        sort_index: i32,
    },
}

impl MetadataType {
    pub(crate) fn sample_name(&self) -> &'static str {
        match *self {
            MetadataType::ProcessName { .. } => "process_name",
            MetadataType::ProcessLabels { .. } => "process_labels",
            MetadataType::ProcessSortIndex { .. } => "process_sort_index",
            MetadataType::ThreadName { .. } => "thread_name",
            MetadataType::ThreadSortIndex { .. } => "thread_sort_index",
        }
    }

    pub(crate) fn consume(self) -> (Option<String>, Option<i32>) {
        match self {
            MetadataType::ProcessName { name } => (Some(name), None),
            MetadataType::ThreadName { name } => (Some(name), None),
            MetadataType::ProcessSortIndex { sort_index } => (None, Some(sort_index)),
            MetadataType::ThreadSortIndex { sort_index } => (None, Some(sort_index)),
            MetadataType::ProcessLabels { .. } => (None, None),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SampleArgs {
    #[serde(rename = "xi_payload")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<TracePayloadT>,

    #[serde(rename = "name")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_name: Option<StrCow>,

    #[serde(rename = "sort_index")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_sort_index: Option<i32>,
}

#[inline]
fn ns_to_us(ns: u64) -> u64 {
    ns / 1000
}

fn serialize_event_type<S>(ph: &SampleEventType, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    s.serialize_char(ph.into_chrome_id())
}

fn deserialize_event_type<'de, D>(d: D) -> Result<SampleEventType, D::Error>
where
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(d).map(SampleEventType::from_chrome_id)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Sample {
    pub name: StrCow,
    #[serde(rename = "cat")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub categories: Option<CategoriesT>,
    #[serde(rename = "ts")]
    pub timestamp_us: u64,
    #[serde(rename = "ph")]
    #[serde(serialize_with = "serialize_event_type")]
    #[serde(deserialize_with = "deserialize_event_type")]
    pub event_type: SampleEventType,
    #[serde(rename = "dur")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_us: Option<u64>,
    pub pid: u64,
    pub tid: u64,
    #[serde(skip_serializing)]
    pub thread_name: Option<StrCow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<SampleArgs>,
}

pub(crate) fn to_cow_str<S>(s: S) -> StrCow
where
    S: Into<StrCow>,
{
    s.into()
}

impl Sample {
    fn thread_name() -> Option<StrCow> {
        let thread = std::thread::current();
        thread.name().map(|s| to_cow_str((*s).to_string()))
    }

    pub fn new_duration_marker<S, C>(
        name: S,
        categories: C,
        payload: Option<TracePayloadT>,
        event_type: SampleEventType,
    ) -> Self
    where
        S: Into<StrCow>,
        C: Into<CategoriesT>,
    {
        Self {
            name: name.into(),
            categories: Some(categories.into()),
            timestamp_us: ns_to_us(time::precise_time_ns()),
            event_type,
            duration_us: None,
            tid: sys_tid::current_tid().unwrap(),
            thread_name: Sample::thread_name(),
            pid: sys_pid::current_pid(),
            args: Some(SampleArgs {
                payload,
                metadata_name: None,
                metadata_sort_index: None,
            }),
        }
    }

    pub fn new_duration<S, C>(
        name: S,
        categories: C,
        payload: Option<TracePayloadT>,
        start_ns: u64,
        duration_ns: u64,
    ) -> Self
    where
        S: Into<StrCow>,
        C: Into<CategoriesT>,
    {
        Self {
            name: name.into(),
            categories: Some(categories.into()),
            timestamp_us: ns_to_us(start_ns),
            event_type: SampleEventType::CompleteDuration,
            duration_us: Some(ns_to_us(duration_ns)),
            tid: sys_tid::current_tid().unwrap(),
            thread_name: Sample::thread_name(),
            pid: sys_pid::current_pid(),
            args: Some(SampleArgs {
                payload,
                metadata_name: None,
                metadata_sort_index: None,
            }),
        }
    }

    pub fn new_instant<S, C>(name: S, categories: C, payload: Option<TracePayloadT>) -> Self
    where
        S: Into<StrCow>,
        C: Into<CategoriesT>,
    {
        Self {
            name: name.into(),
            categories: Some(categories.into()),
            timestamp_us: ns_to_us(time::precise_time_ns()),
            event_type: SampleEventType::Instant,
            duration_us: None,
            tid: sys_tid::current_tid().unwrap(),
            thread_name: Sample::thread_name(),
            pid: sys_pid::current_pid(),
            args: Some(SampleArgs {
                payload,
                metadata_name: None,
                metadata_sort_index: None,
            }),
        }
    }

    pub(crate) fn new_metadata(timestamp_ns: u64, meta: MetadataType, tid: u64) -> Self {
        let sample_name = to_cow_str(meta.sample_name());
        let (metadata_name, sort_index) = meta.consume();

        Self {
            name: sample_name,
            categories: None,
            timestamp_us: ns_to_us(timestamp_ns),
            event_type: SampleEventType::Metadata,
            duration_us: None,
            tid,
            thread_name: None,
            pid: sys_pid::current_pid(),
            args: Some(SampleArgs {
                payload: None,
                metadata_name: metadata_name.map(Cow::Owned),
                metadata_sort_index: sort_index,
            }),
        }
    }
}

impl PartialEq for Sample {
    fn eq(&self, other: &Sample) -> bool {
        self.timestamp_us == other.timestamp_us
            && self.name == other.name
            && self.categories == other.categories
            && self.pid == other.pid
            && self.tid == other.tid
            && self.event_type == other.event_type
            && self.args == other.args
    }
}

impl Eq for Sample {}

impl PartialOrd for Sample {
    fn partial_cmp(&self, other: &Sample) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Sample {
    fn cmp(&self, other: &Sample) -> cmp::Ordering {
        self.timestamp_us.cmp(&other.timestamp_us)
    }
}

impl Hash for Sample {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.pid, self.timestamp_us).hash(state);
    }
}

#[must_use]
pub struct SampleGuard<'a> {
    sample: Option<Sample>,
    trace: Option<&'a Trace>,
}

impl<'a> SampleGuard<'a> {
    #[inline]
    pub fn new_disabled() -> Self {
        Self {
            sample: None,
            trace: None,
        }
    }

    #[inline]
    pub(crate) fn new<S, C>(
        trace: &'a Trace,
        name: S,
        categories: C,
        payload: Option<TracePayloadT>,
    ) -> Self
    where
        S: Into<StrCow>,
        C: Into<CategoriesT>,
    {
        let guard = Self {
            sample: Some(Sample::new_duration_marker(
                name,
                categories,
                payload,
                SampleEventType::DurationBegin,
            )),
            trace: Some(trace),
        };
        trace.record(guard.sample.as_ref().unwrap().clone());
        guard
    }
}

impl<'a> Drop for SampleGuard<'a> {
    fn drop(&mut self) {
        if let Some(ref mut trace) = self.trace {
            let mut sample = self.sample.take().unwrap();
            sample.timestamp_us = ns_to_us(time::precise_time_ns());
            sample.event_type = SampleEventType::DurationEnd;
            trace.record(sample);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cross_process_samples() {
        let mut samples = [
            Sample::new_instant("local pid", &[], None),
            Sample::new_instant("remote pid", &[], None),
        ];
        samples[0].pid = 1;
        samples[0].timestamp_us = 10;

        samples[1].pid = 2;
        samples[1].timestamp_us = 5;

        samples.sort();

        assert_eq!(samples[0].name, "remote pid");
        assert_eq!(samples[1].name, "local pid");
    }
}
