# OpenAPI fuzzer

![ci](https://github.com/matusf/openapi-fuzzer/actions/workflows/ci.yml/badge.svg)

Black-box fuzzer that fuzzes APIs based on [OpenAPI specification](https://github.com/OAI/OpenAPI-Specification/). All you need to do is to supply URL of the API and its specification. Find bugs for free!

![image](https://user-images.githubusercontent.com/18228995/225413315-eab08df2-ed56-4b7a-8c8a-027c18d9a106.png)

## Findings

The fuzzer has been used to find bugs in numerous software. Some of the well-known fuzzed software include[^1]:

<details><summary><b>Kubernetes</b></summary>

- [kubenetes#101350](https://github.com/kubernetes/kubernetes/issues/101350)
- [kubenetes#101348](https://github.com/kubernetes/kubernetes/issues/101348)
- [kubenetes#101355](https://github.com/kubernetes/kubernetes/issues/101355)

</details>

<details><summary><b>Gitea</b></summary>

- [gitea#15357](https://github.com/go-gitea/gitea/issues/15357)
- [gitea#15356](https://github.com/go-gitea/gitea/issues/15356)
- [gitea#15346](https://github.com/go-gitea/gitea/issues/15346)

</details>

<details><summary><b>Vault</b></summary>

- [vault#11310](https://github.com/hashicorp/vault/issues/11310)
- [vault#11311](https://github.com/hashicorp/vault/issues/11311)
- [vault#11313](https://github.com/hashicorp/vault/issues/11313)

</details>

The category of bugs differ, but some of the common are parsing bugs, invalid format bugs and querying non-existent entities. **If you have found bugs with this fuzzer, please reach out to me. I would love to hear from you.** Feel free to submit a PR and add your finding to the list above.

## Building & installing

### From crates.io

To build the fuzzer, you will need to have [rust installed](https://www.rust-lang.org/learn/get-started).

```sh
cargo install openapi-fuzzer
```

### From source

```sh
git clone git@github.com:matusf/openapi-fuzzer.git
cd openapi-fuzzer

# Install to the $PATH
cargo install --path .

# Or build inside the repo
cargo build --release
```

### Using containers

```sh
podman pull ghcr.io/matusf/openapi-fuzzer
```

## Usage

After installation you will have the `openapi-fuzzer` binary available to you, which offers two subcommands - `run` and `resend`.  The `run` subcommand will fuzz the API according to the specification and report any findings. All findings will be stored in a JSON format in a `results` directory (the name of the directory can be specified by `--results-dir` flag).

If the fuzzer finds a bug it will save the seed that leads to the generation of the payload triggering the bug. Those seeds are saved in a regressions file called `openapi-fuzzer.regressions`. The seeds will be used in the next runs of the fuzzer to check if the bug persists. You shall save it alongside your project.

When you are done with fuzzing, you can use `openapi-fuzzer resend` to resend payloads that triggered bugs and examine the cause in depth.

OpenAPI fuzzer supports version 3 of the OpenAPI specification in YAML or JSON format. You can convert older versions at [editor.swagger.io](https://editor.swagger.io/).

### Tips

- When the fuzzer receives an unexpected status code, it will report it as a finding. However, many APIs do not specify client error status codes in the specification. To minimize false positive findings ignore status codes that you are not interested in with `-i` flag. It is advised to fuzz it in two stages. Firstly, run the fuzzer without `-i` flag. Then check the `results` folder for the reported findings. If there are reports from status codes you do not care about, add them via `-i` flag and rerun the fuzzer.
- Most APIs use some base prefix for endpoints like `/v1` or `/api`, however, the specifications are sometimes written without it. Do not forget to **include the path prefix in the url**.
- You may add an extra header with `-H` flag. It may be useful when you would like to increase coverage by providing some sort of authorization. You can use the `-H` flag to add cookies too. e.g. `-H "Cookie: A=1;"`. Use a single `-H` flag when adding multiple cookies as well. e.g. `-H "Cookie: A=1; B=2; C=3;"`.
- Currently, the fuzzer makes 256 requests per endpoint. If all received responses are expected, it declares the endpoint as ok and continues to fuzz the next one. You can adjust this number by setting a `--max-test-case-count` flag.

```console
$ openapi-fuzzer run --help
Usage: openapi-fuzzer run -s <spec> -u <url> [-i <ignore-status-code>] [-H <header>] [--max-test-case-count <max-test-case-count>] [-o <results-dir>] [--stats-dir <stats-dir>]

run openapi-fuzzer

Options:
  -s, --spec        path to OpenAPI specification file
  -u, --url         url of api to fuzz
  -i, --ignore-status-code
                    status codes that will not be considered as finding
  -H, --header      additional header to send
  --max-test-case-count
                    maximum number of test cases that will run for each
                    combination of endpoint and method (default: 256)
  -o, --results-dir directory for results with minimal generated payload used
                    for resending requests (default: results).
  --stats-dir       directory for request times statistics. if no value is
                    supplied, statistics will not be saved
  --help            display usage information

$ openapi-fuzzer run -s ./spec.yaml -u http://127.0.0.1:8200/v1/ -i 404 -i 400 -H  "Authorization: Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
```

### Replaying findings

When you are done fuzzing you can replay the findings. All findings are stored in the `results` folder. Name of each file consists of concatenated endpoint, HTTP method and received status code. To resend the same payload to API, you need to run `openapi-fuzzer resend` and specify a path to the finding file as an argument and a url of the api. You can overwrite the headers with `-H` flag as well, which is useful, when you need authorization.

```console
$ ls -1 results/
api-v1-componentstatuses-{name}-GET-500.json
api-v1-namespaces-{namespace}-configmaps-GET-500.json
api-v1-namespaces-{namespace}-endpoints-GET-500.json
api-v1-namespaces-{namespace}-events-GET-500.json
api-v1-namespaces-{namespace}-limitranges-GET-500.json
api-v1-namespaces-{namespace}-persistentvolumeclaims-GET-500.json
api-v1-namespaces-{namespace}-pods-GET-500.json
api-v1-namespaces-{namespace}-podtemplates-GET-500.json
api-v1-namespaces-{namespace}-replicationcontrollers-GET-500.json
api-v1-namespaces-{namespace}-resourcequotas-GET-500.json
api-v1-namespaces-{namespace}-secrets-GET-500.json
api-v1-namespaces-{namespace}-serviceaccounts-GET-500.json
api-v1-namespaces-{namespace}-services-GET-500.json
api-v1-watch-namespaces-{name}-GET-500.json
...

$ openapi-fuzzer resend --help
Usage: openapi-fuzzer resend <file> [-H <header...>] -u <url>

resend payload genereted by fuzzer

Positional Arguments:
  file              path to result file generated by fuzzer

Options:
  -H, --header      extra header
  -u, --url         url of api
  --help            display usage information

$ openapi-fuzzer resend --url https://minikubeca:8443 results/api-v1-componentstatuses-\{name\}-GET-500.json -H "Authorization: Bearer $KUBE_TOKEN" | jq
500 (Internal Server Error)
{
  "kind": "Status",
  "apiVersion": "v1",
  "metadata": {},
  "status": "Failure",
  "message": "Component not found: ኊ0",
  "code": 500
}
```

[^1]: not all found bugs are linked
