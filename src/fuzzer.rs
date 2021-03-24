use std::fs::{self, File};

use anyhow::{Context, Result};
use arbitrary::{Arbitrary, Unstructured};
use openapi_utils::ReferenceOrExt;
use openapiv3::{
    ArrayType, ObjectType, OpenAPI, Operation, Parameter, PathItem, Responses, SchemaKind,
    StatusCode, Type,
};
use rand::{thread_rng, Rng};
use serde::Serialize;
use serde_json::json;
use std::convert::TryFrom;
use ureq::OrAnyStatus;
use url::Url;

#[derive(Debug, Serialize)]
struct Payload<'a> {
    #[serde(skip)]
    url: &'a Url,
    method: &'a str,
    path: &'a str,
    query_params: Vec<(&'a str, String)>,
    path_params: Vec<(&'a str, String)>,
    headers: Vec<(&'a str, String)>,
    cookies: Vec<(&'a str, String)>,
    body: Vec<serde_json::Value>,
    #[serde(skip)]
    responses: &'a Responses,
}

impl<'a> Payload<'a> {
    fn create(
        url: &'a Url,
        path: &'a str,
        item: &'a PathItem,
        extra_headers: &'a Vec<(String, String)>,
    ) -> Result<Vec<Payload<'a>>> {
        // TODO: Pass parameters to fuzz operation
        let operations = vec![
            ("GET", &item.get),
            ("PUT", &item.put),
            ("POST", &item.post),
            ("DELETE", &item.delete),
            ("OPTIONS", &item.options),
            ("HEAD", &item.head),
            ("PATCH", &item.patch),
            ("TRACE", &item.trace),
        ];

        let mut payloads = Vec::new();
        for (method, op) in operations {
            if let Some(operation) = op {
                payloads.push(Payload::prepare_request(
                    url,
                    method,
                    path,
                    operation,
                    extra_headers,
                )?)
            }
        }

        Ok(payloads)
    }

    fn prepare_request(
        url: &'a Url,
        method: &'a str,
        path: &'a str,
        operation: &'a Operation,
        extra_headers: &'a Vec<(String, String)>,
    ) -> Result<Payload<'a>> {
        let mut query_params: Vec<(&str, String)> = Vec::new();
        let mut path_params: Vec<(&str, String)> = Vec::new();
        let mut headers: Vec<(&str, String)> = Vec::new();
        let mut cookies: Vec<(&str, String)> = Vec::new();

        // Set-up random data generator
        let mut rng = thread_rng();
        let mut arr: Vec<u32> = Vec::with_capacity(rng.gen_range(0..=1024));
        rng.try_fill(&mut arr[..])?;
        let fuzzer_input = arr
            .iter()
            .map(|u| char::try_from(*u))
            .flatten()
            .collect::<String>();

        let mut generator = Unstructured::new(&fuzzer_input.as_bytes());

        for ref_or_param in operation.parameters.iter() {
            match ref_or_param.to_item_ref() {
                Parameter::Query { parameter_data, .. } => {
                    query_params.push((&parameter_data.name, String::arbitrary(&mut generator)?))
                }
                Parameter::Path { parameter_data, .. } => {
                    path_params.push((&parameter_data.name, String::arbitrary(&mut generator)?))
                }
                Parameter::Header { parameter_data, .. } => {
                    headers.push((&parameter_data.name, String::arbitrary(&mut generator)?))
                }
                Parameter::Cookie { parameter_data, .. } => {
                    cookies.push((&parameter_data.name, String::arbitrary(&mut generator)?))
                }
            }
        }

        let body = operation.request_body.as_ref().map(|ref_or_body| {
            let request_body = ref_or_body.to_item_ref();
            request_body
                .content
                .iter()
                .filter(|(content, _)| content.contains("json"))
                .map(|(_, media)| {
                    media.schema.as_ref().map(|schema| {
                        Payload::schema_kind_to_json(
                            &schema.to_item_ref().schema_kind,
                            &mut generator,
                        )
                    })
                })
                .flatten()
                .collect::<Result<Vec<_>>>()
        });

        for (name, value) in extra_headers {
            let index = headers
                .iter()
                .position(|(header_name, _)| header_name == &name);
            match index {
                Some(i) => headers[i] = (&name, value.clone()),
                None => headers.push((&name, value.clone())),
            }
        }

        Ok(Payload {
            url,
            method,
            path,
            query_params,
            path_params,
            headers,
            cookies,
            body: body.unwrap_or(Ok(Vec::new()))?,
            responses: &operation.responses,
        })
    }

    fn generate_json_object(
        object: &ObjectType,
        gen: &mut Unstructured,
    ) -> Result<serde_json::Value> {
        let mut json_object = serde_json::Map::with_capacity(object.properties.len());
        for (name, schema) in &object.properties {
            let schema_kind = &schema.to_item_ref().schema_kind;
            json_object.insert(
                name.clone(),
                Payload::schema_kind_to_json(schema_kind, gen)?,
            );
        }
        Ok(serde_json::Value::Object(json_object))
    }

    fn generate_json_array(array: &ArrayType, gen: &mut Unstructured) -> Result<serde_json::Value> {
        let items = array.items.to_item_ref();
        let (min, max) = (array.min_items.unwrap_or(1), array.max_items.unwrap_or(10));
        let json_array = (min..=max)
            .map(|_| Payload::schema_kind_to_json(&items.schema_kind, gen))
            .collect::<Result<Vec<serde_json::Value>>>();
        Ok(serde_json::Value::Array(json_array?))
    }

    fn schema_type_to_json(
        schema_type: &Type,
        gen: &mut Unstructured,
    ) -> Result<serde_json::Value> {
        match schema_type {
            Type::String(_string_type) => Ok(ureq::json!(String::arbitrary(gen)?)),
            Type::Number(_number_type) => Ok(ureq::json!(f64::arbitrary(gen)?)),
            Type::Integer(_integer_type) => Ok(ureq::json!(i64::arbitrary(gen)?)),
            Type::Object(object_type) => Payload::generate_json_object(object_type, gen),
            Type::Array(array_type) => Payload::generate_json_array(array_type, gen),
            Type::Boolean {} => Ok(ureq::json!(bool::arbitrary(gen)?)),
        }
    }

    fn schema_kind_to_json(
        schema_kind: &SchemaKind,
        gen: &mut Unstructured,
    ) -> Result<serde_json::Value> {
        match schema_kind {
            SchemaKind::Any(_any) => todo!(),
            SchemaKind::Type(schema_type) => Ok(Payload::schema_type_to_json(schema_type, gen)?),
            SchemaKind::OneOf { .. } => todo!(),
            SchemaKind::AnyOf { .. } => todo!(),
            SchemaKind::AllOf { .. } => todo!(),
        }
    }

    fn to_curl(&self) -> Result<String> {
        let mut curl_command = format!("curl -X {} ", self.method);
        if self.body.len() > 0 {
            curl_command += &format!(
                "-d '{}' ",
                serde_json::to_string(&self.body[0]).expect("unable to serialize json")
            );
        }
        for (name, value) in &self.headers {
            curl_command += &format!("-H {}:{} ", name, value);
        }

        let mut path_with_params = self.path.to_owned();
        for (name, value) in self.path_params.iter() {
            path_with_params = path_with_params.replace(&format!("{{{}}}", name), &value);
        }

        Ok(curl_command
            + self
                .url
                .join(&path_with_params.trim_start_matches('/'))?
                .as_str())
    }
}

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
