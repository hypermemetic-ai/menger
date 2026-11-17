//! **A schema made a table** (LOGS-1 › define): which parts of JSON Schema
//! are taken, each field's column, an entry checked against them, and a
//! schema grown by adding only.
//!
//! Taken: a root `{"type": "object"}` with `properties`, `required`,
//! `additionalProperties: false`, `title`, `description`, `$schema`; each
//! property a `type` of `string`, `integer`, `number`, `boolean` (a typed
//! column) or `array`, `object` (a column of JSON, checked only to be that
//! JSON type), with `title`, `description`, and `enum` (strings, on a
//! string). Anything else — `oneOf`, `anyOf`, `allOf`, `not`, `$ref`,
//! `pattern`, bounds, `items`, a nested `properties`, a type list, a root that
//! is not an object, a property named like an envelope column or not a safe
//! identifier — is refused, naming its place in the schema as a JSON
//! pointer (`/properties/value/minimum`), and nothing is made.

use crate::vocab::{identifier_ok, NAME_MAX};
use rusqlite::types::Value as Sql;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The envelope's columns, which the server writes: no field may be named
/// one — nor `log` and `entry`, the names a read across logs answers by, nor
/// SQLite's own `rowid` and `oid`.
pub const RESERVED: &[&str] = &["seq", "at", "by", "tags", "ticket", "log", "entry", "rowid", "oid"];
/// The most fields a log has.
pub const FIELDS_MAX: usize = 200;
/// The most values an `enum` names, and the longest each is.
pub const ENUM_MAX: usize = 256;
/// The longest a description or a title is: 1 KiB.
pub const DESCRIPTION_MAX: usize = 1024;

/// A field's JSON type, and so its column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// `string` — a TEXT column.
    String,
    /// `integer` — an INTEGER column (a whole number within ±2⁶³).
    Integer,
    /// `number` — a REAL column.
    Number,
    /// `boolean` — an INTEGER column, 0 or 1, answered as `true` / `false`.
    Boolean,
    /// `array` — a TEXT column holding the JSON.
    Array,
    /// `object` — a TEXT column holding the JSON.
    Object,
}

impl Kind {
    /// Its JSON Schema `type`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Kind::String => "string",
            Kind::Integer => "integer",
            Kind::Number => "number",
            Kind::Boolean => "boolean",
            Kind::Array => "array",
            Kind::Object => "object",
        }
    }

    /// Its column's SQL type.
    pub const fn sql(self) -> &'static str {
        match self {
            Kind::String | Kind::Array | Kind::Object => "TEXT",
            Kind::Integer | Kind::Boolean => "INTEGER",
            Kind::Number => "REAL",
        }
    }

    /// Its column as a read answers it: `text`, `integer`, `real`, `boolean`, `json`.
    pub const fn column(self) -> &'static str {
        match self {
            Kind::String => "text",
            Kind::Integer => "integer",
            Kind::Number => "real",
            Kind::Boolean => "boolean",
            Kind::Array | Kind::Object => "json",
        }
    }

    fn of(t: &str) -> Option<Kind> {
        Some(match t {
            "string" => Kind::String,
            "integer" => Kind::Integer,
            "number" => Kind::Number,
            "boolean" => Kind::Boolean,
            "array" => Kind::Array,
            "object" => Kind::Object,
            _ => return None,
        })
    }
}

/// One field of a log: its column, as the schema declares it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub kind: Kind,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Closed to new entries, and why (LOGS-1 › deprecate).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
}

/// Why a schema was refused: where in it (a JSON pointer), and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub place: String,
    pub why: String,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "at {}: {}", if self.place.is_empty() { "/" } else { &self.place }, self.why)
    }
}

fn refuse<T>(place: impl Into<String>, why: impl Into<String>) -> Result<T, Refused> {
    Err(Refused { place: place.into(), why: why.into() })
}

/// A property's name as a JSON pointer's segment.
fn segment(name: &str) -> String {
    name.replace('~', "~0").replace('/', "~1")
}

fn words(v: &Value, place: &str) -> Result<Option<String>, Refused> {
    match v {
        Value::String(s) if s.len() <= DESCRIPTION_MAX => Ok(Some(s.clone())),
        Value::String(_) => refuse(place, format!("at most {DESCRIPTION_MAX} bytes")),
        _ => refuse(place, "a string"),
    }
}

const ROOT_KEYS: &[&str] = &["type", "properties", "required", "additionalProperties", "title", "description", "$schema"];
const PROPERTY_KEYS: &[&str] = &["type", "title", "description", "enum"];

/// The fields `text` (a JSON Schema of an object) declares, in its order,
/// or where and why it cannot be a table.
pub fn parse(text: &str) -> Result<Vec<Field>, Refused> {
    let root: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return refuse("", format!("not JSON ({e})")),
    };
    let Value::Object(root) = root else { return refuse("", "a schema is a JSON object, {\"type\": \"object\", \"properties\": {…}}") };
    if let Some(k) = root.keys().find(|k| !ROOT_KEYS.contains(&k.as_str())) {
        return refuse(format!("/{}", segment(k)), format!("`{k}` is not taken (a log's schema takes {})", ROOT_KEYS.join(", ")));
    }
    match root.get("type") {
        Some(Value::String(t)) if t == "object" => {}
        _ => return refuse("/type", "the root is an object: \"type\": \"object\""),
    }
    match root.get("additionalProperties") {
        None | Some(Value::Bool(false)) => {}
        Some(_) => return refuse("/additionalProperties", "only false is taken: an entry with a field the schema does not name is refused anyway"),
    }
    for k in ["title", "description"] {
        if let Some(v) = root.get(k) {
            words(v, &format!("/{k}"))?;
        }
    }
    if root.get("$schema").is_some_and(|v| !v.is_string()) {
        return refuse("/$schema", "a string");
    }
    let empty = Map::new();
    let props = match root.get("properties") {
        None => &empty,
        Some(Value::Object(p)) => p,
        Some(_) => return refuse("/properties", "an object of the fields"),
    };
    if props.is_empty() {
        return refuse("/properties", "a log has at least one field");
    }
    if props.len() > FIELDS_MAX {
        return refuse("/properties", format!("at most {FIELDS_MAX} fields; this names {}", props.len()));
    }
    let order = written_order(text);
    let mut names: Vec<&String> = props.keys().collect();
    names.sort_by_key(|n| order.iter().position(|o| o == *n).unwrap_or(usize::MAX));
    let mut fields = names.into_iter().map(|name| property(name, &props[name])).collect::<Result<Vec<_>, _>>()?;
    required(&root, &mut fields)?;
    Ok(fields)
}

/// The properties' names in the order the schema writes them — a log's
/// columns follow it (serde_json's map keeps its keys sorted).
fn written_order(text: &str) -> Vec<String> {
    struct Keys(Vec<String>);
    impl<'de> Deserialize<'de> for Keys {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            struct V;
            impl<'de> serde::de::Visitor<'de> for V {
                type Value = Keys;
                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("an object")
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Keys, A::Error> {
                    let mut keys = Vec::new();
                    while let Some((k, _)) = map.next_entry::<String, serde::de::IgnoredAny>()? {
                        keys.push(k);
                    }
                    Ok(Keys(keys))
                }
            }
            d.deserialize_map(V)
        }
    }
    #[derive(Deserialize)]
    struct Root {
        properties: Option<Keys>,
    }
    serde_json::from_str::<Root>(text).ok().and_then(|r| r.properties).map_or_else(Vec::new, |k| k.0)
}

fn property(name: &str, p: &Value) -> Result<Field, Refused> {
    let at = format!("/properties/{}", segment(name));
    if !identifier_ok(name) {
        return refuse(&at, format!("`{name}` is not a field's name: a lower-case letter, then lower-case letters, digits and _, at most {NAME_MAX}"));
    }
    if RESERVED.contains(&name) {
        return refuse(&at, format!("`{name}` is the envelope's (the server writes {}): name the field otherwise", RESERVED.join(", ")));
    }
    let Value::Object(p) = p else { return refuse(&at, "a property is an object, {\"type\": …}") };
    if let Some(k) = p.keys().find(|k| !PROPERTY_KEYS.contains(&k.as_str())) {
        return refuse(format!("{at}/{}", segment(k)), format!("`{k}` is not taken (a field takes {}; an array or object is kept as JSON, unchecked inside)", PROPERTY_KEYS.join(", ")));
    }
    let kind = match p.get("type") {
        Some(Value::String(t)) => Kind::of(t).ok_or_else(|| Refused { place: format!("{at}/type"), why: format!("`{t}` is not taken: string, integer, number, boolean, array or object") })?,
        Some(_) => return refuse(format!("{at}/type"), "one type, as a string: string, integer, number, boolean, array or object"),
        None => return refuse(format!("{at}/type"), "a field names its type"),
    };
    if let Some(t) = p.get("title") {
        words(t, &format!("{at}/title"))?;
    }
    let description = p.get("description").map(|d| words(d, &format!("{at}/description"))).transpose()?.flatten();
    let values = p.get("enum").map(|e| values(e, kind, &format!("{at}/enum"))).transpose()?;
    Ok(Field { name: name.to_string(), kind, required: false, values, description, deprecated: None })
}

fn values(e: &Value, kind: Kind, at: &str) -> Result<Vec<String>, Refused> {
    if kind != Kind::String {
        return refuse(at, "`enum` is taken on a string alone");
    }
    let Value::Array(items) = e else { return refuse(at, "an array of strings") };
    if items.is_empty() || items.len() > ENUM_MAX {
        return refuse(at, format!("1 to {ENUM_MAX} strings"));
    }
    let mut out: Vec<String> = Vec::new();
    for (i, v) in items.iter().enumerate() {
        match v {
            Value::String(s) if s.len() <= ENUM_MAX && !out.contains(s) => out.push(s.clone()),
            Value::String(s) if out.contains(s) => return refuse(format!("{at}/{i}"), format!("`{s}` twice")),
            _ => return refuse(format!("{at}/{i}"), format!("a string of at most {ENUM_MAX} bytes")),
        }
    }
    Ok(out)
}

fn required(root: &Map<String, Value>, fields: &mut [Field]) -> Result<(), Refused> {
    let Some(req) = root.get("required") else { return Ok(()) };
    let Value::Array(names) = req else { return refuse("/required", "an array of the fields' names") };
    for (i, n) in names.iter().enumerate() {
        let Value::String(n) = n else { return refuse(format!("/required/{i}"), "a field's name") };
        match fields.iter_mut().find(|f| &f.name == n) {
            Some(f) if f.required => return refuse(format!("/required/{i}"), format!("`{n}` twice")),
            Some(f) => f.required = true,
            None => return refuse(format!("/required/{i}"), format!("`{n}` is no property of the schema")),
        }
    }
    Ok(())
}

/// Why an entry was refused: which field (none: the entry as a whole), why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Misfit {
    pub field: Option<String>,
    pub why: String,
}

fn misfit<T>(field: &str, why: impl Into<String>) -> Result<T, Misfit> {
    Err(Misfit { field: Some(field.to_string()), why: why.into() })
}

/// `entry` checked against `fields`: each field's value as its column
/// keeps it (NULL for one it does not carry), in the fields' order — or the
/// field that does not fit, and why.
pub fn check(fields: &[Field], entry: &Value) -> Result<Vec<Sql>, Misfit> {
    let Value::Object(entry) = entry else { return Err(Misfit { field: None, why: "an entry is a JSON object".into() }) };
    for (k, v) in entry {
        match fields.iter().find(|f| &f.name == k) {
            None => {
                let names: Vec<&str> = fields.iter().filter(|f| f.deprecated.is_none()).map(|f| f.name.as_str()).collect();
                return misfit(k, format!("`{k}` is no field of the log (its fields: {})", names.join(", ")));
            }
            Some(f) => match &f.deprecated {
                Some(why) if !v.is_null() => return misfit(k, format!("`{k}` is deprecated: {why}")),
                _ => {}
            },
        }
    }
    fields
        .iter()
        .map(|f| match entry.get(&f.name) {
            None | Some(Value::Null) if f.required => misfit(&f.name, format!("`{}` is required", f.name)),
            None | Some(Value::Null) => Ok(Sql::Null),
            Some(v) => column(f, v),
        })
        .collect()
}

fn column(f: &Field, v: &Value) -> Result<Sql, Misfit> {
    let wrong = || misfit(&f.name, format!("`{}` is {}, not {}", f.name, f.kind.as_str(), json_type(v)));
    match (f.kind, v) {
        (Kind::String, Value::String(s)) => match &f.values {
            Some(vs) if !vs.contains(s) => misfit(&f.name, format!("`{}` is one of {}, not `{s}`", f.name, vs.join(", "))),
            _ => Ok(Sql::Text(s.clone())),
        },
        (Kind::Integer, Value::Number(n)) => match n.as_i64() {
            Some(i) => Ok(Sql::Integer(i)),
            None if n.is_u64() => misfit(&f.name, format!("`{}` is past the largest integer kept (2⁶³ − 1)", f.name)),
            None => wrong(),
        },
        (Kind::Number, Value::Number(n)) => n.as_f64().map(Sql::Real).ok_or(()).or_else(|_| wrong()),
        (Kind::Boolean, Value::Bool(b)) => Ok(Sql::Integer(*b as i64)),
        (Kind::Array, Value::Array(_)) | (Kind::Object, Value::Object(_)) => Ok(Sql::Text(v.to_string())),
        _ => wrong(),
    }
}

/// A JSON value's type, in JSON Schema's words.
pub fn json_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_f64() => "number",
        Value::Number(_) => "integer",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// A column's value as the entry carried it.
pub fn json_of(kind: Kind, v: Sql) -> Value {
    match (kind, v) {
        (_, Sql::Null) => Value::Null,
        (Kind::Boolean, Sql::Integer(i)) => Value::Bool(i != 0),
        (Kind::Array | Kind::Object, Sql::Text(t)) => serde_json::from_str(&t).unwrap_or(Value::Null),
        (_, Sql::Integer(i)) => Value::from(i),
        (_, Sql::Real(r)) => serde_json::Number::from_f64(r).map_or(Value::Null, Value::Number),
        (_, Sql::Text(t)) => Value::String(t),
        (_, Sql::Blob(_)) => Value::Null,
    }
}

/// `next` (a log's new schema) checked against `now` (its fields): every
/// field kept, of its type, never newly required, its `enum` never
/// narrowed; each new field optional. The fields after — `now`'s in their
/// order (each deprecation kept), then the new ones — and which are new; or
/// the field that breaks the rule, and why.
pub fn grow(now: &[Field], next: Vec<Field>) -> Result<(Vec<Field>, Vec<Field>), Misfit> {
    let mut kept = Vec::with_capacity(next.len());
    for old in now {
        let Some(new) = next.iter().find(|f| f.name == old.name) else {
            return misfit(&old.name, format!("`{}` is dropped: a log's schema grows by adding only (a field renamed is one dropped)", old.name));
        };
        if new.kind != old.kind {
            return misfit(&old.name, format!("`{}` is retyped from {} to {}: a field keeps its type", old.name, old.kind.as_str(), new.kind.as_str()));
        }
        if new.required && !old.required {
            return misfit(&old.name, format!("`{}` is newly required: the entries before it may not carry it", old.name));
        }
        match (&old.values, &new.values) {
            (Some(was), Some(is)) if was.iter().any(|v| !is.contains(v)) => {
                let lost: Vec<&str> = was.iter().filter(|v| !is.contains(v)).map(String::as_str).collect();
                return misfit(&old.name, format!("`{}`'s enum drops {}: an enum grows only", old.name, lost.join(", ")));
            }
            (None, Some(_)) => return misfit(&old.name, format!("`{}` gains an enum: that narrows what it took", old.name)),
            _ => {}
        }
        kept.push(Field { deprecated: old.deprecated.clone(), ..new.clone() });
    }
    let added: Vec<Field> = next.into_iter().filter(|f| !now.iter().any(|o| o.name == f.name)).collect();
    if let Some(f) = added.iter().find(|f| f.required) {
        return misfit(&f.name, format!("`{}` is new and required: a field added to a log is optional (the entries before it have none)", f.name));
    }
    kept.extend(added.iter().cloned());
    Ok((kept, added))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) const PERF: &str = r#"{
      "type": "object",
      "required": ["app", "scenario", "metric", "value", "unit", "commit"],
      "properties": {
        "app": { "type": "string", "description": "what was measured" },
        "scenario": { "type": "string" },
        "transport": { "type": "string", "enum": ["memory", "websocket", "stdio"] },
        "metric": { "type": "string" },
        "value": { "type": "number" },
        "unit": { "type": "string" },
        "commit": { "type": "string" },
        "machine": { "type": "string" },
        "load": { "type": "number" },
        "runs": { "type": "integer" },
        "warm": { "type": "boolean" },
        "config": { "type": "object" },
        "samples": { "type": "array" }
      }
    }"#;

    fn refused(schema: Value) -> Refused {
        parse(&schema.to_string()).expect_err(&schema.to_string())
    }

    /// Scenario "the perf log": each property its column, in its order.
    #[test]
    fn the_perf_schema_is_a_table() {
        let f = parse(PERF).unwrap();
        let cols: Vec<(&str, &str, bool)> = f.iter().map(|f| (f.name.as_str(), f.kind.column(), f.required)).collect();
        assert_eq!(cols[..5], [("app", "text", true), ("scenario", "text", true), ("transport", "text", false), ("metric", "text", true), ("value", "real", true)]);
        assert_eq!(f.iter().find(|f| f.name == "config").unwrap().kind.sql(), "TEXT");
        assert_eq!(f.iter().find(|f| f.name == "warm").unwrap().kind.column(), "boolean");
        assert_eq!(f[2].values.as_deref(), Some(&["memory".to_string(), "websocket".into(), "stdio".into()][..]));
        assert_eq!(f[0].description.as_deref(), Some("what was measured"));
    }

    /// Scenario "a schema it cannot hold": each refused naming its place.
    #[test]
    fn what_is_not_taken_is_refused_naming_its_place() {
        let p = |props: Value| json!({"type": "object", "properties": props});
        let cases = [
            (json!({"oneOf": [{"type": "object"}]}), "/oneOf"),
            (json!({"type": "object", "properties": {"a": {"$ref": "#/x"}}}), "/properties/a/$ref"),
            (json!({"type": "array"}), "/type"),
            (json!([1]), ""),
            (json!({"type": "object", "anyOf": []}), "/anyOf"),
            (json!({"type": "object", "allOf": []}), "/allOf"),
            (p(json!({"a": {"type": "string", "pattern": "x"}})), "/properties/a/pattern"),
            (p(json!({"a": {"type": "number", "minimum": 1}})), "/properties/a/minimum"),
            (p(json!({"a": {"type": "array", "items": {"type": "string"}}})), "/properties/a/items"),
            (p(json!({"a": {"type": "object", "properties": {}}})), "/properties/a/properties"),
            (p(json!({"a": {"type": ["string", "null"]}})), "/properties/a/type"),
            (p(json!({"a": {"type": "null"}})), "/properties/a/type"),
            (p(json!({"a": {}})), "/properties/a/type"),
            (p(json!({"seq": {"type": "integer"}})), "/properties/seq"),
            (p(json!({"tags": {"type": "array"}})), "/properties/tags"),
            (p(json!({"Value": {"type": "number"}})), "/properties/Value"),
            (p(json!({"a\"; DROP TABLE logs; --": {"type": "number"}})), "/properties/a\"; DROP TABLE logs; --"),
            (p(json!({"a/b": {"type": "number"}})), "/properties/a~1b"),
            (p(json!({"a": {"type": "integer", "enum": ["1"]}})), "/properties/a/enum"),
            (p(json!({})), "/properties"),
            (json!({"type": "object", "properties": {"a": {"type": "string"}}, "required": ["b"]}), "/required/0"),
            (json!({"type": "object", "properties": {"a": {"type": "string"}}, "additionalProperties": true}), "/additionalProperties"),
        ];
        for (schema, place) in cases {
            let r = refused(schema.clone());
            assert_eq!(r.place, place, "{schema}: {r}");
        }
        assert!(parse("not json").unwrap_err().why.starts_with("not JSON"));
        let many: Map<String, Value> = (0..=FIELDS_MAX).map(|i| (format!("f{i}"), json!({"type": "string"}))).collect();
        assert_eq!(refused(json!({"type": "object", "properties": many})).place, "/properties");
    }

    /// Scenario "a field of the wrong type", and the rest of the check.
    #[test]
    fn an_entry_is_checked_field_by_field() {
        let f = parse(PERF).unwrap();
        let good = json!({"app": "plexus", "scenario": "s", "metric": "items_per_s", "value": 12.5, "unit": "1/s", "commit": "plexus@abc", "runs": 3, "warm": true, "config": {"threads": 4}, "transport": "websocket"});
        let row = check(&f, &good).unwrap();
        assert_eq!(row.len(), f.len());
        assert_eq!(row[4], Sql::Real(12.5));
        assert_eq!(row[7], Sql::Null, "machine absent");
        assert_eq!(row[9], Sql::Integer(3));
        assert_eq!(row[10], Sql::Integer(1));
        assert_eq!(row[11], Sql::Text(r#"{"threads":4}"#.into()));
        let with = |k: &str, v: Value| {
            let mut e = good.clone();
            e[k] = v;
            check(&f, &e).unwrap_err()
        };
        assert_eq!(with("value", json!("fast")).field.as_deref(), Some("value"));
        assert!(with("value", json!("fast")).why.contains("number, not string"));
        let mut whole = good.clone();
        whole["value"] = json!(7);
        assert_eq!(check(&f, &whole).unwrap()[4], Sql::Real(7.0), "an integer is a number");
        assert_eq!(with("runs", json!(1.5)).field.as_deref(), Some("runs"));
        assert_eq!(with("runs", json!(u64::MAX)).why, "`runs` is past the largest integer kept (2⁶³ − 1)");
        assert_eq!(with("warm", json!(1)).field.as_deref(), Some("warm"));
        assert_eq!(with("config", json!([1])).field.as_deref(), Some("config"));
        assert!(with("transport", json!("carrier pigeon")).why.contains("one of memory, websocket, stdio"));
        assert!(with("app", Value::Null).why.contains("required"));
        assert!(with("nope", json!(1)).why.contains("no field of the log"));
        assert!(check(&f, &json!([1])).unwrap_err().field.is_none());
        let mut gone = good.clone();
        gone.as_object_mut().unwrap().remove("commit");
        assert_eq!(check(&f, &gone).unwrap_err().field.as_deref(), Some("commit"));
        // A deprecated field: refused when carried, null or absent kept.
        let mut d = f.clone();
        d[7].deprecated = Some("one machine now".into());
        assert_eq!(check(&d, &with_machine(&good)).unwrap_err().why, "`machine` is deprecated: one machine now");
        assert!(check(&d, &good).is_ok());
    }

    fn with_machine(e: &Value) -> Value {
        let mut e = e.clone();
        e["machine"] = json!("box");
        e
    }

    /// Scenarios "a new field" and "a field taken away".
    #[test]
    fn a_schema_grows_by_adding_only() {
        let now = parse(PERF).unwrap();
        let next = |edit: &dyn Fn(&mut Value)| {
            let mut s: Value = serde_json::from_str(PERF).unwrap();
            edit(&mut s);
            grow(&now, parse(&s.to_string()).unwrap())
        };
        let (all, added) = next(&|s| s["properties"]["p99"] = json!({"type": "number"})).unwrap();
        assert_eq!((all.len(), added.len(), added[0].name.as_str()), (now.len() + 1, 1, "p99"));
        assert_eq!(all.last().unwrap().name, "p99");
        let refused = |edit: &dyn Fn(&mut Value)| next(edit).unwrap_err();
        assert!(refused(&|s| {
            s["properties"].as_object_mut().unwrap().remove("machine");
        })
        .why
        .contains("`machine` is dropped"));
        assert!(refused(&|s| s["properties"]["load"] = json!({"type": "string"})).why.contains("retyped from number to string"));
        assert_eq!(refused(&|s| s["required"].as_array_mut().unwrap().push(json!("machine"))).field.as_deref(), Some("machine"));
        assert!(refused(&|s| {
            s["properties"]["p99"] = json!({"type": "number"});
            s["required"].as_array_mut().unwrap().push(json!("p99"));
        })
        .why
        .contains("new and required"));
        assert!(refused(&|s| s["properties"]["transport"]["enum"] = json!(["memory"])).why.contains("drops websocket, stdio"));
        assert!(refused(&|s| s["properties"]["machine"]["enum"] = json!(["a"])).why.contains("gains an enum"));
        assert!(next(&|s| s["properties"]["transport"]["enum"] = json!(["memory", "websocket", "stdio", "quic"])).is_ok(), "an enum grows");
        let renamed = refused(&|s| {
            let m = s["properties"].as_object_mut().unwrap().remove("machine").unwrap();
            s["properties"]["host"] = m;
        });
        assert_eq!(renamed.field.as_deref(), Some("machine"), "a rename is a drop");
        let mut d = now.clone();
        d[7].deprecated = Some("x".into());
        let (kept, _) = grow(&d, parse(PERF).unwrap()).unwrap();
        assert_eq!(kept[7].deprecated.as_deref(), Some("x"), "a deprecation stays");
    }
}
