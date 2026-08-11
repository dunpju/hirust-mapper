//! P9 `#[derive(MapperModel)]` 测试：列映射内省。

use hirust_mapper_macros::MapperModel;

#[derive(MapperModel)]
#[allow(dead_code)]
struct User {
    #[mapper(column = "user_name")]
    name: String,
    age: i64,
    #[mapper(column = "created_at", type_handler = "chrono::DateTime<chrono::Utc>")]
    created: String,
    no_attr: bool,
}

#[test]
fn column_mappings_default_and_override() {
    let mappings = User::column_mappings();
    // 声明 column 的使用声明的值
    assert!(mappings.contains(&("name", "user_name")));
    // 未声明 column 的默认为字段名
    assert!(mappings.contains(&("age", "age")));
    assert!(mappings.contains(&("no_attr", "no_attr")));
}

#[test]
fn type_handlers_collected() {
    let handlers = User::type_handlers();
    assert_eq!(handlers, &[("created", "chrono::DateTime<chrono::Utc>")]);
    // name/age/no_attr 未声明 type_handler → 不出现
    assert!(!handlers.iter().any(|(f, _)| *f == "name"));
}

#[test]
fn empty_struct_mappings() {
    #[derive(MapperModel)]
    struct Empty;
    assert!(Empty::column_mappings().is_empty());
    assert!(Empty::type_handlers().is_empty());
}
