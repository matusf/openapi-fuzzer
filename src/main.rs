mod arbitrary;
mod fuzzer;

use anyhow::{Context, Result};
use argh::FromArgs;
use fuzzer::Fuzzer;
use openapi_utils::SpecExt;
use openapiv3::OpenAPI;
use std::path::PathBuf;
use std::str::FromStr;
use url::{ParseError, Url};

#[derive(FromArgs, Debug)]
/// OpenAPI fuzzer
struct Args {
    /// path to OpenAPI specification file
    #[argh(option, short = 's')]
    spec: PathBuf,

    /// url of api to fuzz
    #[argh(option, short = 'u')]
    url: UrlWithTrailingSlash,

    /// status codes that will not be considered as finding
    #[argh(option, short = 'i')]
    ignore_status_code: Vec<u16>,

    /// additional header to send
    #[argh(option, short = 'H')]
    header: Vec<Header>,
}

#[derive(Debug)]
struct Header(String, String);

impl FromStr for Header {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts = s.splitn(2, ':').collect::<Vec<_>>();
        if parts.len() != 2 {
            return Err("invalid header format".to_string());
        }
        Ok(Header(
            parts[0].to_string().to_lowercase(),
            parts[1].to_string(),
        ))
    }
}

impl From<Header> for (String, String) {
    fn from(val: Header) -> Self {
        (val.0, val.1)
    }
}

#[derive(Debug)]
struct UrlWithTrailingSlash(Url);

impl FromStr for UrlWithTrailingSlash {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.ends_with('/') {
            true => Ok(UrlWithTrailingSlash(Url::from_str(s)?)),
            false => Ok(UrlWithTrailingSlash(Url::from_str(&(s.to_owned() + "/"))?)),
        }
    }
}

impl From<UrlWithTrailingSlash> for Url {
    fn from(val: UrlWithTrailingSlash) -> Self {
        val.0
    }
}

fn main() -> Result<()> {
    let args: Args = argh::from_env();
    let specfile = std::fs::read_to_string(&args.spec)?;
    let openapi_schema: OpenAPI =
        serde_yaml::from_str(&specfile).context("Failed to parse schema")?;
    let openapi_schema = openapi_schema.deref_all();

    Fuzzer::new(
        openapi_schema,
        args.url.into(),
        args.ignore_status_code,
        args.header.into_iter().map(Into::into).collect(),
    )
    .run();

    Ok(())
}
