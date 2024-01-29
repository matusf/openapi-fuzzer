use std::{iter::FromIterator, rc::Rc};

use openapi_utils::ReferenceOrExt;
use openapiv3::{
    ArrayType, ObjectType, Operation, Parameter, ParameterData, ParameterSchemaOrContent,
    SchemaKind, Type,
};

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
        Type::Boolean(_) => any::<bool>().prop_map_into::<serde_json::Value>().boxed(),
        Type::Integer(_integer_type) => any::<i64>().prop_map_into::<serde_json::Value>().boxed(),
        Type::Number(_number_type) => any::<f32>().prop_map_into::<serde_json::Value>().boxed(),
        Type::String(_string_type) => any::<String>().prop_map_into::<serde_json::Value>().boxed(),
        Type::Object(object_type) => generate_json_object(object_type),
        Type::Array(array_type) => generate_json_array(array_type),
    }
}

fn schema_kind_to_json(schema_kind: &SchemaKind) -> BoxedStrategy<serde_json::Value> {
    match schema_kind {
        SchemaKind::Any(_any) => any::<String>().prop_map_into::<serde_json::Value>().boxed(),
        SchemaKind::Not { not: schema } => {
            schema_kind_to_json(&schema.to_item_ref().schema_kind).boxed()
        }
        SchemaKind::Type(schema_type) => schema_type_to_json(schema_type).boxed(),
        // TODO: AllOf should generate all schemas and merge them to one json object
        SchemaKind::AllOf { all_of: schemas }
        | SchemaKind::AnyOf { any_of: schemas }
        | SchemaKind::OneOf { one_of: schemas } => Union::new(
            schemas
                .iter()
                .map(|ref_of_schema| schema_kind_to_json(&ref_of_schema.to_item_ref().schema_kind)),
        )
        .boxed(),
    }
}

fn any_json(schema_kind: &SchemaKind) -> impl Strategy<Value = serde_json::Value> {
    schema_kind_to_json(schema_kind)
}

fn parameter_data_to_strategy(
    parameter_data: &ParameterData,
    string_strategy: impl Strategy<Value = String> + 'static,
) -> (Just<String>, impl Strategy<Value = String>) {
    let ParameterSchemaOrContent::Schema(schema) = &parameter_data.format else {
        return (Just(parameter_data.name.clone()), string_strategy.boxed());
    };

    let SchemaKind::Type(schema_type) = &schema.to_item_ref().schema_kind else {
        return (Just(parameter_data.name.clone()), string_strategy.boxed());
    };

    let value = match &schema_type {
        Type::Boolean(_) => any::<bool>().prop_map(|i| i.to_string()).boxed(),
        Type::Integer(_integer_type) => any::<i64>().prop_map(|i| i.to_string()).boxed(),
        Type::Number(_number_type) => any::<f32>().prop_map(|i| i.to_string()).boxed(),
        _ => string_strategy.boxed(),
    };

    (Just(parameter_data.name.clone()), value)
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
struct Parameters {
    headers: Vec<(String, String)>,
    path: Vec<(String, String)>,
    query: Vec<(String, String)>,
}

impl Arbitrary for Parameters {
    type Parameters = Rc<ArbitraryParameters>;

    fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
        let mut headers = vec![];
        let mut path_parameters = vec![];
        let mut query_parameters = vec![];

        args.operation.parameters.iter().for_each(|ref_or_param| {
            match ref_or_param {
                Parameter::Header { parameter_data, .. } => {
                    // Generate headers following the HTTP/1.1 RFC
                    // https://datatracker.ietf.org/doc/html/rfc7230#section-3.2
                    headers.push(parameter_data_to_strategy(parameter_data, "[!-~ \t]*"));
                }
                Parameter::Query { parameter_data, .. } => {
                    query_parameters.push(parameter_data_to_strategy(parameter_data, ".*"));
                }
                Parameter::Path { parameter_data, .. } => {
                    path_parameters.push(parameter_data_to_strategy(parameter_data, ".*"));
                }
                Parameter::Cookie { .. } => {}
            };
        });

        (headers, path_parameters, query_parameters)
            .prop_map(|(headers, path, query)| Parameters {
                headers,
                path,
                query,
            })
            .boxed()
    }

    type Strategy = BoxedStrategy<Parameters>;
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Payload {
    parameters: Parameters,
    body: OptionalJSON,
}

impl Arbitrary for Payload {
    type Parameters = Rc<ArbitraryParameters>;
    type Strategy = BoxedStrategy<Payload>;

    fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
        let args = args;
        any_with::<(Parameters, OptionalJSON)>((args.clone(), args))
            .prop_map(|(parameters, body)| Payload { parameters, body })
            .boxed()
    }
}

impl Payload {
    pub fn query_params(&self) -> &[(String, String)] {
        &self.parameters.query
    }

    pub fn path_params(&self) -> &[(String, String)] {
        &self.parameters.path
    }

    pub fn headers(&self) -> &[(String, String)] {
        &self.parameters.headers
    }

    pub fn body(&self) -> Option<&serde_json::Value> {
        self.body.0.as_ref()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use anyhow::Result;
    use indexmap::indexmap;
    use openapiv3::{
        BooleanType, HeaderStyle, IntegerType, NumberType, ParameterData, ParameterSchemaOrContent,
        PathStyle, QueryStyle, ReferenceOr, Schema, SchemaData, StringType,
    };
    use proptest::{
        prop_assert, proptest,
        test_runner::{Config, FileFailurePersistence, TestError, TestRunner},
    };

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

    enum ParameterType {
        Query,
        Header,
        Path,
    }

    fn create_parameter(
        parameter_type: ParameterType,
        name: &str,
        schema_kind: Option<SchemaKind>,
    ) -> ReferenceOr<Parameter> {
        let format = match schema_kind {
            Some(schema_kind) => ParameterSchemaOrContent::Schema(ReferenceOr::Item(Schema {
                schema_data: SchemaData::default(),
                schema_kind: schema_kind,
            })),
            None => ParameterSchemaOrContent::Content(Default::default()),
        };

        match parameter_type {
            ParameterType::Query => ReferenceOr::Item(Parameter::Query {
                parameter_data: ParameterData {
                    name: name.into(),
                    description: None,
                    required: false,
                    deprecated: None,
                    format,
                    example: None,
                    examples: Default::default(),
                    explode: None,
                    extensions: Default::default(),
                },
                style: QueryStyle::Form,
                allow_reserved: false,
                allow_empty_value: None,
            }),
            ParameterType::Header => ReferenceOr::Item(Parameter::Header {
                parameter_data: ParameterData {
                    name: name.into(),
                    description: None,
                    required: false,
                    deprecated: None,
                    format,
                    example: None,
                    examples: Default::default(),
                    explode: None,
                    extensions: Default::default(),
                },
                style: HeaderStyle::Simple,
            }),
            ParameterType::Path => ReferenceOr::Item(Parameter::Path {
                parameter_data: ParameterData {
                    name: name.into(),
                    description: None,
                    required: false,
                    deprecated: None,
                    format,
                    example: None,
                    examples: Default::default(),
                    explode: None,
                    extensions: Default::default(),
                },
                style: PathStyle::Simple,
            }),
        }
    }

    fn create_parameters() -> BoxedStrategy<Parameters> {
        let operation = Operation {
            parameters: vec![
                create_parameter(ParameterType::Header, "string-header", None),
                create_parameter(ParameterType::Path, "string-path", None),
                create_parameter(
                    ParameterType::Path,
                    "float",
                    Some(SchemaKind::Type(Type::Number(NumberType::default()))),
                ),
                create_parameter(
                    ParameterType::Path,
                    "int",
                    Some(SchemaKind::Type(Type::Integer(IntegerType::default()))),
                ),
                create_parameter(
                    ParameterType::Path,
                    "bool",
                    Some(SchemaKind::Type(Type::Boolean(BooleanType::default()))),
                ),
                create_parameter(
                    ParameterType::Query,
                    "float",
                    Some(SchemaKind::Type(Type::Number(NumberType::default()))),
                ),
                create_parameter(
                    ParameterType::Query,
                    "int",
                    Some(SchemaKind::Type(Type::Integer(IntegerType::default()))),
                ),
                create_parameter(
                    ParameterType::Query,
                    "bool",
                    Some(SchemaKind::Type(Type::Boolean(BooleanType::default()))),
                ),
            ],
            ..Default::default()
        };
        Parameters::arbitrary_with(Rc::new(ArbitraryParameters { operation }))
    }

    fn is_valid_header_value_char(b: u8) -> bool {
        match b {
            b' ' | b'\t' | 33..=126 => true,
            _ => false,
        }
    }

    proptest! {
        #[test]
        fn test_parameters(parameters in create_parameters()) {
            for (name, value) in parameters.path.into_iter().chain(parameters.headers).chain(parameters.query) {
                if name == "float" {
                    prop_assert!(value.parse::<f32>().is_ok());
                }
                if name == "int" {
                    prop_assert!(value.parse::<i64>().is_ok());
                }
                if name == "bool" {
                    prop_assert!(value.parse::<bool>().is_ok());
                }
                if name == "string-header"{
                    prop_assert!(value.bytes().all(is_valid_header_value_char));
                }
            }
        }
    }
}
