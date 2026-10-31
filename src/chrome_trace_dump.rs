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
    clippy::if_same_then_else,
    clippy::needless_bool,
    clippy::needless_pass_by_value,
    clippy::ptr_arg
)]

use std::io::{Error as IOError, ErrorKind as IOErrorKind, Read, Write};

use super::Sample;

#[derive(Debug)]
pub enum Error {
    Io(IOError),
    Json(serde_json::Error),
    DecodingFormat(String),
}

impl From<IOError> for Error {
    fn from(e: IOError) -> Error {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Error {
        Error::Json(e)
    }
}

impl From<String> for Error {
    fn from(e: String) -> Error {
        Error::DecodingFormat(e)
    }
}

impl Error {
    pub fn already_exists() -> Error {
        Error::Io(IOError::from(IOErrorKind::AlreadyExists))
    }
}

pub fn serialize<W>(samples: &Vec<Sample>, output: W) -> Result<(), Error>
where
    W: Write,
{
    serde_json::to_writer(output, samples).map_err(Error::Json)
}

pub fn to_value(samples: &Vec<Sample>) -> Result<serde_json::Value, Error> {
    serde_json::to_value(samples).map_err(Error::Json)
}

pub fn decode(samples: serde_json::Value) -> Result<Vec<Sample>, Error> {
    serde_json::from_value(samples).map_err(Error::Json)
}

pub fn deserialize<R>(input: R) -> Result<Vec<Sample>, Error>
where
    R: Read,
{
    serde_json::from_reader(input).map_err(Error::Json)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "json_payload")]
    use crate::TracePayloadT;
    #[cfg(any(feature = "json_payload", feature = "chrome_trace_event"))]
    use serde_json::json;

    #[cfg(not(feature = "json_payload"))]
    fn to_payload(value: &'static str) -> &'static str {
        value
    }

    #[cfg(feature = "json_payload")]
    fn to_payload(value: &'static str) -> TracePayloadT {
        json!({ "test": value })
    }

    #[cfg(feature = "chrome_trace_event")]
    #[test]
    fn test_chrome_trace_serialization() {
        use super::super::*;

        let trace = Trace::enabled(Config::with_limit_count(10));
        trace.instant("sample1", &["test", "chrome"]);
        trace.instant_payload("sample2", &["test", "chrome"], to_payload("payload 2"));
        trace.instant_payload("sample3", &["test", "chrome"], to_payload("payload 3"));
        trace.closure_payload(
            "sample4",
            &["test", "chrome"],
            || {
                let _guard = trace.block("sample5", &["test,chrome"]);
            },
            to_payload("payload 4"),
        );

        let samples = trace.samples_cloned_unsorted();

        let mut serialized = Vec::<u8>::new();

        let result = serialize(&samples, &mut serialized);
        assert!(result.is_ok(), "{:?}", result);

        let decoded_result: Vec<serde_json::Value> = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(decoded_result.len(), 8);
        assert_eq!(decoded_result[0]["name"].as_str().unwrap(), "process_name");
        assert_eq!(decoded_result[1]["name"].as_str().unwrap(), "thread_name");
        for i in 2..5 {
            assert_eq!(decoded_result[i]["name"].as_str().unwrap(), samples[i].name);
            assert_eq!(decoded_result[i]["cat"].as_str().unwrap(), "test,chrome");
            assert_eq!(decoded_result[i]["ph"].as_str().unwrap(), "i");
            assert_eq!(decoded_result[i]["ts"], samples[i].timestamp_us);
            let nth_sample = &samples[i];
            let nth_args = nth_sample.args.as_ref().unwrap();
            assert_eq!(
                decoded_result[i]["args"]["xi_payload"],
                json!(nth_args.payload.as_ref())
            );
        }
        assert_eq!(decoded_result[5]["ph"], "B");
        assert_eq!(decoded_result[6]["ph"], "E");
        assert_eq!(decoded_result[7]["ph"], "X");
    }

    #[cfg(feature = "chrome_trace_event")]
    #[test]
    fn test_chrome_trace_deserialization() {
        use super::super::*;

        let trace = Trace::enabled(Config::with_limit_count(10));
        trace.instant("sample1", &["test", "chrome"]);
        trace.instant_payload("sample2", &["test", "chrome"], to_payload("payload 2"));
        trace.instant_payload("sample3", &["test", "chrome"], to_payload("payload 3"));
        trace.closure_payload(
            "sample4",
            &["test", "chrome"],
            || (),
            to_payload("payload 4"),
        );

        let samples = trace.samples_cloned_unsorted();

        let mut serialized = Vec::<u8>::new();
        let result = serialize(&samples, &mut serialized);
        assert!(result.is_ok(), "{:?}", result);

        let deserialized_samples = deserialize(serialized.as_slice()).unwrap();
        assert_eq!(deserialized_samples, samples);
    }
}
