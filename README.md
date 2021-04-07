# OpenAPI fuzzer

Black-box fuzzer that fuzzes APIs based on [OpenAPI specification](https://github.com/OAI/OpenAPI-Specification/). All you need to do is to supply URL of the API and its specification.

![demo](./demo.png)

## Building & installing

To build the fuzzer, you will need to have [rust installed](https://www.rust-lang.org/learn/get-started).

```sh
# Install from crates.io
cargo install openapi-fuzzer

# Or download the repo and build locally
git clone git@github.com:matusf/openapi-fuzzer.git
cd openapi-fuzzer

# Install to the $PATH
cargo install --path .

# Or build (add --releade to build optimized binary) inside the repo
cargo build
```

## Usage

After installation you will have two binaries in your `$PATH`. `openapi-fuzzer` will fuzz the API according to the specification and report any findings. All findings will be located in a `results` directory and

### Tips

- When the fuzzer receives an unexpected status code, it will report is as a finding. However, many APIs do not specify client error status codes in the specification. To minimize false positive findings ignore status codes that you are not interested in with `-i` flag.
- Most APIs use some base prefix for endpoints like `/v1` or `/api`. The specification is writen without it. Do not forget to **include the path prefix in the url**
- You may add an extra header with `-H` flag. It may be useful when you would like to increase coverage by providing some sort of authorization.

```txt
$ openapi-fuzzer --help
Usage: openapi-fuzzer -s <spec> -u <url> [-i <ignore-status-code>] [-H <header>]

OpenAPI fuzzer

Options:
  -s, --spec        path to OpenAPI specification
  -u, --url         url of api to fuzz
  -i, --ignore-status-code
                    status codes that will not be considered as finding
  -H, --header      additional header to send
  --help            display usage information


$ openapi-fuzzer -s spec.yaml -u http://127.0.0.1:8200/v1/ -i 404
```

### Replaying findings

When you are done fuzzing you can replay the findings. All findings are stored in the `results` folder in path according to finding's endpoint and method. To resend the same payload to API, you simply run `openapi-fuzzer-resender` with path to the finding file as an argument. You can overwrite the headers with `-H` flag as well, which is useful for example, when the authorization token expired.

```txt
$ tree -L 3 results/
results/
├── sys-leases-renew
│   └── POST
│       └── 500
└── sys-seal
    └── POST
        └── 500

$ openapi-fuzzer-resender --help
Usage: openapi-fuzzer-resender <file> [-H <header>]

Resender of openapi-fuzzer results

Options:
  -H, --header      extra header
  --help            display usage information

$ openapi-fuzzer-resender results/sys-seal/POST/500/1b4e8a77.json
Response[status: 500, status_text: Internal Server Error, url: http://127.0.0.1:8200/v1/sys/seal]
"{\"errors\":[\"1 error occurred:\\n\\t* missing client token\\n\\n\"]}\n"
```
