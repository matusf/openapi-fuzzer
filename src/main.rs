use anyhow::{Context, Result};
use argh::FromArgs;
use openapi_utils::{ReferenceOrExt, SpecExt};
use openapiv3::*;
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

fn send_request(url: &Url, path: &str, item: &PathItem) -> Result<()> {
    let path = url.join(path)?;
    let response = item
        .get
        .as_ref()
        .map(|operation| ureq::get(&path.to_string()).call());

    println!("{:?}", response);
    Ok(())
}

fn main() -> Result<()> {
    let args: Args = argh::from_env();
    let specfile = std::fs::read_to_string(&args.spec)?;
    let schema: OpenAPI = serde_yaml::from_str(&specfile).context("Failed to parse schema")?;

    for (path, ref_or_item) in schema.deref_all().paths.iter() {
        send_request(&args.url, path, ref_or_item.to_item_ref())?
    }
    Ok(())
}
