use std::fs::{self, File};

use anyhow::{Context, Result};
use openapi_utils::ReferenceOrExt;
use openapiv3::{OpenAPI, StatusCode};
use serde_json::json;
use ureq::OrAnyStatus;
use url::Url;

use crate::payload::Payload;

#[derive(Debug)]
pub struct Fuzzer {
    schema: OpenAPI,
    url: Url,
    ignored_status_codes: Vec<u16>,
    extra_headers: Vec<(String, String)>,
}

impl Fuzzer {
    pub fn new(
        schema: OpenAPI,
        url: Url,
        ignored_status_codes: Vec<u16>,
        extra_headers: Vec<(String, String)>,
    ) -> Fuzzer {
        Fuzzer {
            schema,
            url,
            ignored_status_codes,
            extra_headers,
        }
    }

    pub fn run(&self) -> Result<()> {
        loop {
            eprint!(".");
            for (path, ref_or_item) in self.schema.paths.iter() {
                let item = ref_or_item.to_item_ref();
                for payload in Payload::create(&self.url, path, item, &self.extra_headers)? {
                    match self.send_request(&payload) {
                        Ok(resp) => self.check_response(&resp, &payload)?,
                        Err(e) => eprintln!("Err sending req: {}", e),
                    };
                }
            }
        }
    }

    fn send_request(&self, payload: &Payload) -> Result<ureq::Response> {
        let mut path_with_params = payload.path.to_owned();
        for (name, value) in payload.path_params.iter() {
            path_with_params = path_with_params.replace(&format!("{{{}}}", name), &value);
        }
        let mut request = ureq::request_url(
            payload.method,
            &payload
                .url
                .join(&path_with_params.trim_start_matches('/'))?,
        );

        for (param, value) in payload.query_params.iter() {
            request = request.query(param, &value)
        }

        for (header, value) in payload.headers.iter() {
            request = request.set(header, &value)
        }

        if payload.body.len() > 0 {
            Ok(request.send_json(payload.body[0].clone()).or_any_status()?)
        } else {
            request.call().or_any_status().map_err(|e| e.into())
        }
    }

    fn check_response(&self, resp: &ureq::Response, payload: &Payload) -> Result<()> {
        let responses = &payload.responses.responses;

        // known non 500 and ingored status codes are OK
        if self.ignored_status_codes.contains(&resp.status())
            || (responses.contains_key(&StatusCode::Code(resp.status()))
                && resp.status() / 100 != 5)
        {
            return Ok(());
        }

        let results_dir = format!(
            "results/{}/{}/{}",
            payload.path.trim_matches('/').replace("/", "-"),
            payload.method,
            resp.status()
        );
        let results_file = format!("{}/{}", results_dir, format!("{:x}", rand::random::<u32>()));
        fs::create_dir_all(&results_dir)?;

        serde_json::to_writer_pretty(
            &File::create(&results_file).context(format!("unable to create {}", &results_file))?,
            &json!({ "url": &payload.url.as_str() ,"payload": payload, "curl": payload.to_curl()?}),
        )
        .map_err(|e| e.into())
    }
}
