use anyhow::{Context, Result};
use arbitrary::{Arbitrary, Unstructured};
use argh::FromArgs;
use openapi_utils::{ReferenceOrExt, SpecExt};
use openapiv3::*;
use rand::Rng;
use serde::Serialize;
use serde_json::json;
use std::{convert::TryFrom, str::FromStr};
use std::{fs, fs::File, path::PathBuf};
use ureq::OrAnyStatus;
use url::Url;

#[derive(FromArgs, Debug)]
/// OpenAPI fuzzer
struct Args {
    /// path to OpenAPI specification
    #[argh(option, short = 's')]
    spec: PathBuf,

    /// url of api to fuzz
    #[argh(option, short = 'u')]
    url: Url,

    /// status codes that will not be considered as finding
    #[argh(option, short = 'i')]
    ignored_status_codes: Vec<u16>,

    /// additional header to send
    #[argh(option, short = 'H')]
    headers: Vec<CLIHeader>,
}

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

#[derive(Debug)]
struct CLIHeader {
    name: String,
    value: String,
}

impl FromStr for CLIHeader {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts = s.splitn(2, ':').collect::<Vec<_>>();
        if parts.len() != 2 {
            return Err("invalid header format".to_string());
        }
        Ok(CLIHeader {
            name: parts[0].to_string(),
            value: parts[1].to_string(),
        })
    }
}

fn send_request(payload: &Payload) -> Result<ureq::Response> {
    let mut path_with_params = payload.path.to_owned();
    for (name, value) in payload.path_params.iter() {
        path_with_params = path_with_params.replace(&format!("{{{}}}", name), &value);
    }

    let mut request = ureq::request_url(payload.method, &payload.url.join(&path_with_params)?);

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

fn generate_json_object(object: &ObjectType, gen: &mut Unstructured) -> Result<serde_json::Value> {
    let mut json_object = serde_json::Map::with_capacity(object.properties.len());
    for (name, schema) in &object.properties {
        let schema_kind = &schema.to_item_ref().schema_kind;
        json_object.insert(name.clone(), schema_kind_to_json(schema_kind, gen)?);
    }
    Ok(serde_json::Value::Object(json_object))
}

fn generate_json_array(array: &ArrayType, gen: &mut Unstructured) -> Result<serde_json::Value> {
    let items = array.items.to_item_ref();
    let (min, max) = (array.min_items.unwrap_or(1), array.max_items.unwrap_or(10));
    let json_array = (min..=max)
        .map(|_| schema_kind_to_json(&items.schema_kind, gen))
        .collect::<Result<Vec<serde_json::Value>>>();
    Ok(serde_json::Value::Array(json_array?))
}

fn schema_type_to_json(schema_type: &Type, gen: &mut Unstructured) -> Result<serde_json::Value> {
    match schema_type {
        Type::String(_string_type) => Ok(ureq::json!(String::arbitrary(gen)?)),
        Type::Number(_number_type) => Ok(ureq::json!(f64::arbitrary(gen)?)),
        Type::Integer(_integer_type) => Ok(ureq::json!(i64::arbitrary(gen)?)),
        Type::Object(object_type) => generate_json_object(object_type, gen),
        Type::Array(array_type) => generate_json_array(array_type, gen),
        Type::Boolean {} => Ok(ureq::json!(bool::arbitrary(gen)?)),
    }
}

fn schema_kind_to_json(
    schema_kind: &SchemaKind,
    gen: &mut Unstructured,
) -> Result<serde_json::Value> {
    match schema_kind {
        SchemaKind::Any(_any) => todo!(),
        SchemaKind::Type(schema_type) => Ok(schema_type_to_json(schema_type, gen)?),
        SchemaKind::OneOf { .. } => todo!(),
        SchemaKind::AnyOf { .. } => todo!(),
        SchemaKind::AllOf { .. } => todo!(),
    }
}

fn prepare_request<'a>(
    url: &'a Url,
    method: &'a str,
    path: &'a str,
    operation: &'a Operation,
    additional_headers: &'a Vec<CLIHeader>,
) -> Result<Payload<'a>> {
    let mut query_params: Vec<(&str, String)> = Vec::new();
    let mut path_params: Vec<(&str, String)> = Vec::new();
    let mut headers: Vec<(&str, String)> = Vec::new();
    let mut cookies: Vec<(&str, String)> = Vec::new();

    // Set-up random data generator
    let mut arr = [0u32; 1024];
    rand::thread_rng().try_fill(&mut arr[..])?;
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
                    schema_kind_to_json(&schema.to_item_ref().schema_kind, &mut generator)
                })
            })
            .flatten()
            .collect::<Result<Vec<_>>>()
    });

    for header in additional_headers {
        let index = headers.iter().position(|(name, _)| name == &header.name);
        match index {
            Some(i) => headers[i] = (&header.name, header.value.clone()),
            None => headers.push((&header.name, header.value.clone())),
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

fn construct_curl_request(payload: &Payload) -> Result<String> {
    let mut curl_command = format!("curl -X {} ", payload.method);
    if payload.body.len() > 0 {
        curl_command += &format!(
            "-d '{}' ",
            serde_json::to_string(&payload.body[0]).expect("unable to serialize json")
        );
    }
    for (name, value) in &payload.headers {
        curl_command += &format!("-H {}:{} ", name, value);
    }

    let mut path_with_params = payload.path.to_owned();
    for (name, value) in payload.path_params.iter() {
        path_with_params = path_with_params.replace(&format!("{{{}}}", name), &value);
    }

    Ok(curl_command + payload.url.join(&path_with_params)?.as_str())
}

fn check_response(
    resp: &ureq::Response,
    payload: &Payload,
    ignored_status_codes: &Vec<u16>,
) -> Result<()> {
    let responses = &payload.responses.responses;

    // known non 500 and ingored status codes are OK
    if ignored_status_codes.contains(&resp.status())
        || (responses.contains_key(&StatusCode::Code(resp.status())) && resp.status() / 100 != 5)
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
        &json!({ "url": &payload.url.as_str() ,"payload": payload, "curl": construct_curl_request(&payload)?}),
    )
    .map_err(|e| e.into())
}

fn create_fuzz_payload<'a>(
    url: &'a Url,
    path: &'a str,
    item: &'a PathItem,
    additional_headers: &'a Vec<CLIHeader>,
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
            payloads.push(prepare_request(
                url,
                method,
                path,
                operation,
                additional_headers,
            )?)
        }
    }

    Ok(payloads)
}

fn main() -> Result<()> {
    let args: Args = argh::from_env();
    let specfile = std::fs::read_to_string(&args.spec)?;
    let openapi_schema: OpenAPI =
        serde_yaml::from_str(&specfile).context("Failed to parse schema")?;
    let openapi_schema = openapi_schema.deref_all();

    loop {
        eprint!(".");
        for (path, ref_or_item) in openapi_schema.paths.iter() {
            let item = ref_or_item.to_item_ref();
            for payload in create_fuzz_payload(&args.url, path, item, &args.headers)? {
                match send_request(&payload) {
                    Ok(resp) => check_response(&resp, &payload, &args.ignored_status_codes)?,
                    Err(e) => eprintln!("Err sending req: {}", e),
                };
            }
        }
    }
}
