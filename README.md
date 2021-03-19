# OpenAPI fuzzer

Black-box fuzzer that fuzzes APIs based on [OpenAPI specification](https://github.com/OAI/OpenAPI-Specification/). All you need to do is to supply URL of the API and its specification.

## Building & Running

To build the fuzzer, you will need to have [rust installed](https://www.rust-lang.org/learn/get-started).

```sh
git clone git@github.com:matusf/open-api-fuzzer.git
cd open-api-fuzzer
# build (for optimized version add --release flag)
cargo build
./target/debug/open-api-fuzzer -u https://url-of-api-to.fuzz -s ./open-api-spec.yml

# build (for optimized version add --release flag) and run
cargo run -- -u https://url-of-api-to.fuzz -s ./open-api-spec.yml
```
