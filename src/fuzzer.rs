use std::{
    borrow::Cow,
    collections::HashMap,
    fs::{self, File},
    mem,
    path::PathBuf,
    rc::Rc,
};

use anyhow::{Context, Result};
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

use crate::arbitrary::{ArbitraryParameters, Payload};

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
}

impl Fuzzer {
    pub fn new(
        schema: OpenAPI,
        url: Url,
        ignored_status_codes: Vec<u16>,
        extra_headers: HashMap<String, String>,
        max_test_case_count: u32,
        output_dir: PathBuf,
    ) -> Fuzzer {
        Fuzzer {
            schema,
            url,
            extra_headers,
            ignored_status_codes,
            max_test_case_count,
            results_dir: output_dir.join("results"),
            stats_dir: output_dir.join("stats"),
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

                let result = TestRunner::new(config.clone()).run(
                    &any_with::<Payload>(Rc::new(ArbitraryParameters::new(operation))),
                    |payload| {
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

                        match self.is_expected_response(&response, &responses) {
                            true => Ok(()),
                            false => Err(TestCaseError::Fail(response.status().to_string().into())),
                        }
                    },
                );

                match result {
                    Err(TestError::Fail(reason, payload)) => {
                        let reason: Cow<str> = reason.message().into();
                        let status_code = reason
                            .parse::<u16>()
                            .map_err(|_| anyhow::Error::msg(reason.into_owned()))?;

                        println!("{method:7} {path_with_params:max_path_length$} failed ");
                        self.save_finding(path_with_params, method, payload, &status_code)?;
                    }
                    Ok(()) => {
                        println!("{method:7} {path_with_params:max_path_length$}   ok   ")
                    }
                    Err(TestError::Abort(_)) => {
                        println!("{method:7} {path_with_params:max_path_length$} aborted")
                    }
                }
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

        for (header, value) in payload.headers().iter() {
            let value = extra_headers.get(&header.to_lowercase()).unwrap_or(value);
            request = request.set(header, value);
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
}
