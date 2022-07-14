#![allow(dead_code)]

use std::iter::FromIterator;

use openapi_utils::ReferenceOrExt;
use openapiv3::{ArrayType, ObjectType, SchemaKind, Type};

use proptest::{
    arbitrary::any,
    collection::vec,
    strategy::{BoxedStrategy, Just, Strategy},
};

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
        _ => unimplemented!(),
    }
}

fn any_json(schema_kind: &SchemaKind) -> impl Strategy<Value = serde_json::Value> {
    schema_kind_to_json(schema_kind)
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
                println!("Found minimal failing case: {}", value);
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
                println!("Found minimal failing case: {}", value);
                Ok(())
            }
            result => panic!("Unexpected result: {:?}", result),
        }
    }
}
