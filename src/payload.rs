use anyhow::{Error, Result};
use arbitrary::{Arbitrary, Unstructured};
use openapi_utils::ReferenceOrExt;
use openapiv3::{
    ArrayType, ObjectType, Operation, Parameter, PathItem, ReferenceOr, Responses, Schema,
    SchemaKind, Type,
};
use rand::{prelude::IteratorRandom, Rng};
use serde::Serialize;
use serde_json::json;
use url::Url;

#[derive(Debug, Serialize)]
pub struct Payload<'a> {
    pub url: &'a Url,
    pub method: &'a str,
    pub path: &'a str,
    pub query_params: Vec<(&'a str, String)>,
    pub path_params: Vec<(&'a str, String)>,
    pub headers: Vec<(&'a str, String)>,
    pub body: Vec<serde_json::Value>,
    #[serde(skip)]
    pub responses: &'a Responses,
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
        Type::String(_string_type) => Ok(json!(String::arbitrary(gen)?)),
        Type::Number(_number_type) => Ok(json!(f64::arbitrary(gen)?)),
        Type::Integer(_integer_type) => Ok(json!(i64::arbitrary(gen)?)),
        Type::Object(object_type) => generate_json_object(object_type, gen),
        Type::Array(array_type) => generate_json_array(array_type, gen),
        Type::Boolean {} => Ok(json!(bool::arbitrary(gen)?)),
    }
}

fn schema_kind_to_json(
    schema_kind: &SchemaKind,
    gen: &mut Unstructured,
) -> Result<serde_json::Value> {
    let f = |vec: &Vec<ReferenceOr<Schema>>,
             gen: &mut Unstructured|
     -> Result<Vec<serde_json::Value>> {
        vec.iter()
            .map(|ref_of_schema| schema_kind_to_json(&ref_of_schema.to_item_ref().schema_kind, gen))
            .collect()
    };

    match schema_kind {
        SchemaKind::Any(_any) => todo!(),
        SchemaKind::Type(schema_type) => Ok(schema_type_to_json(schema_type, gen)?),
        SchemaKind::OneOf { one_of } => f(one_of, gen)?
            .into_iter()
            .choose(&mut rand::thread_rng())
            .ok_or(Error::msg("unable to generate JSON")),
        SchemaKind::AnyOf { any_of } => Ok(f(any_of, gen)?
            .into_iter()
            .choose_multiple(&mut rand::thread_rng(), 5)
            .into()),
        SchemaKind::AllOf { all_of } => Ok(f(all_of, gen)?.into()),
    }
}

impl<'a> Payload<'a> {
    fn new(
        url: &'a Url,
        method: &'a str,
        path: &'a str,
        operation: &'a Operation,
        extra_headers: &'a Vec<(String, String)>,
    ) -> Result<Payload<'a>> {
        let mut query_params: Vec<(&str, String)> = Vec::new();
        let mut path_params: Vec<(&str, String)> = Vec::new();
        let mut headers: Vec<(&str, String)> = Vec::new();

        // Set-up random data generator
        let fuzzer_input: String = rand::thread_rng()
            .sample_iter::<char, _>(rand::distributions::Standard)
            .take(1024)
            .collect();

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
                Parameter::Cookie { parameter_data, .. } => headers.push((
                    "Cookie",
                    format!(
                        "{}={}",
                        parameter_data.name,
                        String::arbitrary(&mut generator)?
                    ),
                )),
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

        for (name, value) in extra_headers {
            let index = headers
                .iter()
                .position(|(header_name, _)| &header_name.to_lowercase() == name);
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
            body: body.unwrap_or(Ok(Vec::new()))?,
            responses: &operation.responses,
        })
    }

    pub fn for_all_methods(
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
                payloads.push(Payload::new(url, method, path, operation, extra_headers)?)
            }
        }

        Ok(payloads)
    }

    pub fn to_curl(&self) -> Result<String> {
        let mut curl_command = format!("curl -X {} ", self.method);
        if self.body.len() > 0 {
            curl_command += &format!(
                "-d '{}' ",
                serde_json::to_string(&self.body[0]).expect("unable to serialize json")
            );
        }
        for (name, value) in &self.headers {
            curl_command += &format!("-H '{}:{}' ", name, value);
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
