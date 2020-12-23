use argh::FromArgs;
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

fn main() {
    let args: Args = argh::from_env();
    println!("{:?}", args);
}
