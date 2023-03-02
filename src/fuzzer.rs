use std::{
    borrow::Cow,
    cell::RefCell,
    collections::HashMap,
    fs::{self, File},
    mem,
    path::PathBuf,
    rc::Rc,
    time::Instant,
};

use anyhow::{Context, Error, Result};
use indexmap::IndexMap;
use openapi_utils::ReferenceOrExt;
use openapiv3::{OpenAPI, ReferenceOr, Response, StatusCode};
use proptest::{
    prelude::any_with,
    test_runner::{Config, FileFailurePersistence, TestCaseError, TestError, TestRunner},
};
use serde::{Deserialize, Serialize};
use ureq::OrAnyStatus;
use url::Url;

use crate::{
    arbitrary::{ArbitraryParameters, Payload},
    stats::Stats,
};

#[derive(Debug, Deserialize, Serialize)]
pub struct FuzzResult<'a> {
    pub payload: Payload,
    pub path: &'a str,
    pub method: &'a str,
}

#[derive(Debug)]
pub struct Fuzzer {
    schema: OpenAPI,
    url: Url,
    ignored_status_codes: Vec<u16>,
    extra_headers: HashMap<String, String>,
    max_test_case_count: u32,
    results_dir: PathBuf,
    stats_dir: PathBuf,
    save_stats: bool,
}

impl Fuzzer {
    pub fn new(
        schema: OpenAPI,
        url: Url,
        ignored_status_codes: Vec<u16>,
        extra_headers: HashMap<String, String>,
        max_test_case_count: u32,
        output_dir: PathBuf,
        save_stats: bool,
    ) -> Fuzzer {
        Fuzzer {
            schema,
            url,
            extra_headers,
            ignored_status_codes,
            max_test_case_count,
            results_dir: output_dir.join("results"),
            stats_dir: output_dir.join("stats"),
            save_stats,
        }
    }

    pub fn run(&mut self) -> Result<()> {
        fs::create_dir_all(&self.results_dir).context(format!(
            "Unable to create directory: {:?}",
            self.results_dir
        ))?;
        fs::create_dir_all(&self.stats_dir)
            .context(format!("Unable to create directory: {:?}", self.stats_dir))?;

        let config = Config {
            failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
                "openapi-fuzzer.regressions",
            ))),
            verbose: 0,
            cases: self.max_test_case_count,
            ..Config::default()
        };
        let paths = mem::take(&mut self.schema.paths);
        let max_path_length = paths.iter().map(|(path, _)| path.len()).max().unwrap_or(0);

        println!("\x1B[1mMETHOD  {path:max_path_length$} STATUS   MEAN (μs) STD.DEV. MIN (μs)   MAX (μs)\x1B[0m",
            path = "PATH"
        );
        for (path_with_params, mut ref_or_item) in paths {
            let path_with_params = path_with_params.trim_start_matches('/');
            let item = ref_or_item.to_item_mut();
            let operations = vec![
                ("GET", item.get.take()),
                ("PUT", item.put.take()),
                ("POST", item.post.take()),
                ("DELETE", item.delete.take()),
                ("OPTIONS", item.options.take()),
                ("HEAD", item.head.take()),
                ("PATCH", item.patch.take()),
                ("TRACE", item.trace.take()),
            ];

            for (method, mut operation) in operations
                .into_iter()
                .filter_map(|(method, operation)| operation.map(|operation| (method, operation)))
            {
                let responses = mem::take(&mut operation.responses.responses);

                let times = RefCell::new(vec![]);

                let result = TestRunner::new(config.clone()).run(
                    &any_with::<Payload>(Rc::new(ArbitraryParameters::new(operation))),
                    |payload| {
                        let now = Instant::now();
                        let response = Fuzzer::send_request(
                            &self.url,
                            path_with_params.to_owned(),
                            method,
                            &payload,
                            &self.extra_headers,
                        )
                        .map_err(|e| {
                            TestCaseError::Fail(format!("unable to send request: {e}").into())
                        })?;

                        times.borrow_mut().push(now.elapsed().as_micros());

                        match self.is_expected_response(&response, &responses) {
                            true => Ok(()),
                            false => Err(TestCaseError::Fail(response.status().to_string().into())),
                        }
                    },
                );
                let times = times.into_inner();
                if self.save_stats {
                    self.save_stats(path_with_params, method, &times)?;
                }
                self.report_run(method, path_with_params, result, max_path_length, &times)?
            }
        }

        Ok(())
    }

    pub fn send_request(
        url: &Url,
        mut path_with_params: String,
        method: &str,
        payload: &Payload,
        extra_headers: &HashMap<String, String>,
    ) -> Result<ureq::Response> {
        for (name, value) in payload.path_params().iter() {
            path_with_params = path_with_params.replace(&format!("{{{name}}}"), value);
        }
        let mut request = ureq::request_url(method, &url.join(&path_with_params)?);

        for (param, value) in payload.query_params().iter() {
            request = request.query(param, value)
        }

        // Add headers overriding genereted ones with extra headers from command line
        for (header, value) in payload.headers().iter() {
            let value = extra_headers.get(&header.to_lowercase()).unwrap_or(value);
            request = request.set(header, value);
        }

        // Add remaining extra headers
        for (header, value) in extra_headers.iter() {
            if request.header(header).is_none() {
                request = request.set(header, value);
            }
        }

        match payload.body() {
            Some(json) => request.send_json(json.clone()),
            None => request.call(),
        }
        .or_any_status()
        .map_err(Into::into)
    }

    fn is_expected_response(
        &self,
        resp: &ureq::Response,
        responses: &IndexMap<StatusCode, ReferenceOr<Response>>,
    ) -> bool {
        // known non 500 and ingored status codes are OK
        self.ignored_status_codes.contains(&resp.status())
            || (responses.contains_key(&StatusCode::Code(resp.status()))
                && resp.status() / 100 != 5)
    }

    fn save_finding(
        &self,
        path: &str,
        method: &str,
        payload: Payload,
        status_code: &u16,
    ) -> Result<()> {
        let file = format!(
            "{}-{method}-{status_code}.json",
            path.trim_matches('/').replace('/', "-")
        );
        serde_json::to_writer_pretty(
            &File::create(self.results_dir.join(&file))
                .context(format!("Unable to create file: {file:?}"))?,
            &FuzzResult {
                payload,
                path,
                method,
            },
        )
        .map_err(|e| e.into())
    }

    fn save_stats(&self, path: &str, method: &str, times: &[u128]) -> Result<()> {
        let file = format!("{}-{method}.json", path.trim_matches('/').replace('/', "-"));

        serde_json::to_writer(
            &File::create(self.stats_dir.join(&file))
                .context(format!("Unable to create file: {file:?}"))?,
            times,
        )
        .map_err(|e| e.into())
    }

    fn report_run(
        &self,
        method: &str,
        path_with_params: &str,
        result: Result<(), TestError<Payload>>,
        max_path_length: usize,
        times: &[u128],
    ) -> Result<()> {
        let status = match result {
            Err(TestError::Fail(reason, payload)) => {
                let reason: Cow<str> = reason.message().into();
                let status_code = reason
                    .parse::<u16>()
                    .map_err(|_| Error::msg(reason.into_owned()))?;

                self.save_finding(path_with_params, method, payload, &status_code)?;
                "failed"
            }
            Ok(()) => "ok",
            Err(TestError::Abort(_)) => "aborted",
        };

        let Stats {
            min,
            max,
            mean,
            std_dev,
        } = Stats::compute(times).ok_or(Error::msg("no requests sent"))?;
        println!("{method:7} {path_with_params:max_path_length$} {status:^7} {mean:10.0} {std_dev:8.0} {min:8} {max:10}");
        Ok(())
    }
}
