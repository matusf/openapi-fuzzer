use std::{iter::FromIterator, rc::Rc};

use openapi_utils::ReferenceOrExt;
use openapiv3::{ArrayType, ObjectType, Operation, Parameter, SchemaKind, Type};

use proptest::{
    arbitrary::any,
    collection::vec,
    prelude::{any_with, Arbitrary},
    strategy::{BoxedStrategy, Just, Strategy, Union},
};
use serde::{Deserialize, Serialize};

pub struct ArbitraryParameters {
    operation: Operation,
}

impl ArbitraryParameters {
    pub fn new(operation: Operation) -> Self {
        ArbitraryParameters { operation }
    }
}

impl Default for ArbitraryParameters {
    fn default() -> Self {
        panic!("no default value for `ArbitraryParameters`")
    }
}

fn generate_json_object(object: &ObjectType) -> BoxedStrategy<serde_json::Value> {
    let mut vec = Vec::with_capacity(object.properties.len());
    for (name, schema) in &object.properties {
        let schema_kind = &schema.to_item_ref().schema_kind;
        vec.push((Just(name.clone()), schema_kind_to_json(schema_kind)));
    }
    vec.prop_map(|vec| serde_json::Value::Object(serde_json::Map::from_iter(vec)))
        .boxed()
}

fn generate_json_array(array: &ArrayType) -> BoxedStrategy<serde_json::Value> {
    let items = array.items.to_item_ref();
    let (min, max) = (array.min_items.unwrap_or(1), array.max_items.unwrap_or(10));
    vec(schema_kind_to_json(&items.schema_kind), (min, max))
        .prop_map(serde_json::Value::Array)
        .boxed()
}

fn schema_type_to_json(schema_type: &Type) -> BoxedStrategy<serde_json::Value> {
    match schema_type {
        Type::Boolean {} => any::<bool>().prop_map(Into::into).boxed(),
        Type::Integer(_integer_type) => any::<i64>().prop_map(Into::into).boxed(),
        Type::Number(_number_type) => any::<f32>().prop_map(Into::into).boxed(),
        Type::String(_string_type) => any::<String>().prop_map(Into::into).boxed(),
        Type::Object(object_type) => generate_json_object(object_type),
        Type::Array(array_type) => generate_json_array(array_type),
    }
}

fn schema_kind_to_json(schema_kind: &SchemaKind) -> BoxedStrategy<serde_json::Value> {
    match schema_kind {
        SchemaKind::Any(_any) => any::<String>().prop_map(serde_json::Value::String).boxed(),
        SchemaKind::Type(schema_type) => schema_type_to_json(schema_type).boxed(),
        SchemaKind::OneOf { one_of } => Union::new(
            one_of
                .iter()
                .map(|ref_of_schema| schema_kind_to_json(&ref_of_schema.to_item_ref().schema_kind)),
        )
        .boxed(),
        _ => unimplemented!(),
    }
}

fn any_json(schema_kind: &SchemaKind) -> impl Strategy<Value = serde_json::Value> {
    schema_kind_to_json(schema_kind)
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct OptionalJSON(Option<serde_json::Value>);

impl Arbitrary for OptionalJSON {
    type Parameters = Rc<ArbitraryParameters>;

    fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
        if let Some(ref_or_body) = args.operation.request_body.as_ref() {
            let request_body = ref_or_body.to_item_ref();
            for (media_type_name, media_type) in &request_body.content {
                if media_type_name.contains("json") {
                    match media_type
                        .schema
                        .as_ref()
                        .map(|schema| any_json(&schema.to_item_ref().schema_kind))
                    {
                        Some(strategy) => {
                            return strategy.prop_map(|json| OptionalJSON(Some(json))).boxed();
                        }
                        None => continue,
                    };
                };
            }
        };

        Just(OptionalJSON(None)).boxed()
    }

    type Strategy = BoxedStrategy<OptionalJSON>;
}

#[derive(Debug, Deserialize, Serialize)]
struct Headers(Vec<(String, String)>);

impl Arbitrary for Headers {
    type Parameters = Rc<ArbitraryParameters>;

    fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
        args.operation
            .parameters
            .iter()
            .flat_map(|ref_or_param| match ref_or_param.to_item_ref() {
                Parameter::Header { parameter_data, .. } => {
                    Some((Just(parameter_data.name.clone()), any::<String>()))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .prop_map(Headers)
            .boxed()
    }
    type Strategy = BoxedStrategy<Headers>;
}

#[derive(Debug, Deserialize, Serialize)]
struct PathParams(Vec<(String, String)>);

impl Arbitrary for PathParams {
    type Parameters = Rc<ArbitraryParameters>;

    fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
        let mut path_params = vec![];
        for ref_or_param in args.operation.parameters.iter() {
            match ref_or_param.to_item_ref() {
                Parameter::Path { parameter_data, .. } => {
                    path_params.push((Just(parameter_data.name.clone()), any::<String>()))
                }
                _ => continue,
            }
        }
        path_params.prop_map(PathParams).boxed()
    }
    type Strategy = BoxedStrategy<PathParams>;
}

#[derive(Debug, Deserialize, Serialize)]
struct QueryParams(Vec<(String, String)>);

impl Arbitrary for QueryParams {
    type Parameters = Rc<ArbitraryParameters>;

    fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
        let mut query_params = vec![];
        for ref_or_param in args.operation.parameters.iter() {
            match ref_or_param.to_item_ref() {
                Parameter::Query { parameter_data, .. } => {
                    query_params.push((Just(parameter_data.name.clone()), any::<String>()))
                }
                _ => continue,
            }
        }
        query_params.prop_map(QueryParams).boxed()
    }
    type Strategy = BoxedStrategy<QueryParams>;
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Payload {
    query_params: QueryParams,
    path_params: PathParams,
    headers: Headers,
    body: OptionalJSON,
    // TODO: add cookies
}

impl Arbitrary for Payload {
    type Parameters = Rc<ArbitraryParameters>;
    type Strategy = BoxedStrategy<Payload>;

    fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
        let args = args;
        any_with::<(QueryParams, PathParams, Headers, OptionalJSON)>((
            args.clone(),
            args.clone(),
            args.clone(),
            args,
        ))
        .prop_map(|(query_params, path_params, headers, body)| Payload {
            query_params,
            path_params,
            headers,
            body,
        })
        .boxed()
    }
}

impl Payload {
    pub fn query_params(&self) -> &[(String, String)] {
        &self.query_params.0
    }

    pub fn path_params(&self) -> &[(String, String)] {
        &self.path_params.0
    }

    pub fn headers(&self) -> &[(String, String)] {
        &self.headers.0
    }

    pub fn body(&self) -> Option<&serde_json::Value> {
        match &self.body.0 {
            Some(json) => Some(json),
            None => None,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use anyhow::Result;
    use indexmap::indexmap;
    use openapiv3::*;
    use proptest::test_runner::{Config, FileFailurePersistence, TestError, TestRunner};

    #[test]
    fn test_json_string() {
        let mut runner = TestRunner::new(Config {
            failure_persistence: Some(Box::new(FileFailurePersistence::Off)),
            ..Config::default()
        });

        let result = runner.run(
            &any_json(&SchemaKind::Type(Type::String(StringType::default()))),
            |s| {
                if let serde_json::Value::String(str) = s {
                    assert!(!serde_json::from_str::<String>(&str).unwrap().is_empty())
                }
                Ok(())
            },
        );

        match result {
            Err(TestError::Fail(_, value)) => {
                println!("Found minimal failing case: {value}");
            }
            result => panic!("Unexpected result: {:?}", result),
        }
    }

    #[test]
    fn test_json_object() -> Result<()> {
        let mut runner = TestRunner::new(Config {
            failure_persistence: Some(Box::new(FileFailurePersistence::Off)),
            ..Config::default()
        });

        let s: SchemaKind = SchemaKind::Type(Type::Object(ObjectType {
            properties: indexmap! {
                "date".to_string() => ReferenceOr::Item(Box::new(Schema {
                    schema_kind: SchemaKind::Type(Type::String(StringType::default())),
                    schema_data: Default::default(),
                })),
                "temperatureC".to_string() => ReferenceOr::Item(Box::new(Schema {
                    schema_kind: SchemaKind::Type(Type::Integer(IntegerType::default())),
                    schema_data: Default::default(),
                })),
            },
            ..Default::default()
        }));

        let result = runner.run(&any_json(&s), |obj| {
            if let serde_json::Value::Object(map) = obj {
                assert!(map.get("temperatureC").unwrap().as_i64() >= Some(0));
            }
            Ok(())
        });

        match result {
            Err(TestError::Fail(_, value)) => {
                println!("Found minimal failing case: {value}");
                Ok(())
            }
            result => panic!("Unexpected result: {:?}", result),
        }
    }
}
