use anyhow::{Context, Result};
use arbitrary::{Arbitrary, Unstructured};
use argh::FromArgs;
use openapi_utils::{ReferenceOrExt, SpecExt};
use openapiv3::{Parameter, *};
use rand::{thread_rng, Rng};
use std::path::PathBuf;
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
}

fn fuzz_operation(url: &Url, path: &str, operation: &Operation) -> Result<()> {
    let mut query_params: Vec<(&str, String)> = Vec::new();
    let mut path_params: Vec<(&str, String)> = Vec::new();
    let mut headers: Vec<(&str, String)> = Vec::new();
    let mut cookies: Vec<(&str, String)> = Vec::new();

    // Set-up random data generator
    let mut seed = [0u8, 42];
    thread_rng().fill(&mut seed[..]);
    let mut generator = Unstructured::new(&seed);

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

    if let Some(ref_or_body) = operation.request_body.as_ref() {
        let body = ref_or_body.to_item_ref();
        println!("{:?}", body)
    }

    let mut path_with_params = path.to_owned();
    for (name, value) in dbg!(path_params) {
        println!("{:?}", path_with_params);
        println!("{:?} = {:?}", format!("{{{}}}", name), value);
        path_with_params = path_with_params.replace(&format!("{{{}}}", name), &value);
        println!("{:?}", path_with_params);
    }

    let mut request = ureq::get(&url.join(&path_with_params)?.to_string());

    for (param, value) in query_params {
        request = request.query(param, &value)
    }

    for (header, value) in headers {
        request = request.set(header, &value)
    }

    println!("{:?}", request);
    Ok(())
}

fn fuzz_path(url: &Url, path: &str, item: &PathItem) -> Result<()> {
    if let Some(operation) = item.get.as_ref() {
        fuzz_operation(url, path, operation)?
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Args = argh::from_env();
    let specfile = std::fs::read_to_string(&args.spec)?;
    let schema: OpenAPI = serde_yaml::from_str(&specfile).context("Failed to parse schema")?;

    for (path, ref_or_item) in schema.deref_all().paths.iter() {
        fuzz_path(&args.url, path, ref_or_item.to_item_ref())?
    }
    Ok(())
}
