//! Writes a migrated record with its keys in the order its file had them,
//! so a migrated file's diff shows only the fields that changed.

use serde::de::{Deserialize, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use serde_json::Value;

/// The key order of a JSON document: each object's keys as its file listed
/// them, and each array's items.
#[derive(Debug, Default)]
enum Order {
    #[default]
    Leaf,
    Array(Vec<Order>),
    Object(Vec<(String, Order)>),
}

/// A field a rule renamed, which takes the old field's place.
const RENAMED: [(&str, &str); 1] = [("session", "by")];

/// `value` as pretty JSON with a final newline, its keys in the order
/// `original` lists them. A key `original` lacks follows the ones it has.
pub(crate) fn text(value: &Value, original: &str) -> String {
    let order = serde_json::from_str(original).unwrap_or_default();
    let arranged = Arranged {
        value,
        order: &order,
    };
    serde_json::to_string_pretty(&arranged).unwrap_or_default() + "\n"
}

struct Arranged<'a> {
    value: &'a Value,
    order: &'a Order,
}

const LEAF: &Order = &Order::Leaf;

impl Serialize for Arranged<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (self.value, self.order) {
            (Value::Object(map), Order::Object(keys)) => {
                let mut placed: Vec<(&str, &Order)> = Vec::new();
                for (key, order) in keys {
                    let renamed = RENAMED.iter().find(|(old, _)| old == key);
                    if map.contains_key(key) {
                        placed.push((key, order));
                    } else if let Some((_, new)) = renamed
                        && map.contains_key(*new)
                        && !keys.iter().any(|(listed, _)| listed == new)
                    {
                        placed.push((new, LEAF));
                    }
                }
                let rest = map
                    .keys()
                    .filter(|key| !placed.iter().any(|(at, _)| at == key));
                let rest: Vec<(&str, &Order)> = rest.map(|key| (key.as_str(), LEAF)).collect();
                let mut out = serializer.serialize_map(Some(map.len()))?;
                for (key, order) in placed.into_iter().chain(rest) {
                    let value = &map[key];
                    out.serialize_entry(key, &Arranged { value, order })?;
                }
                out.end()
            }
            (Value::Array(items), Order::Array(orders)) => {
                let mut out = serializer.serialize_seq(Some(items.len()))?;
                for (at, value) in items.iter().enumerate() {
                    let order = orders.get(at).unwrap_or(LEAF);
                    out.serialize_element(&Arranged { value, order })?;
                }
                out.end()
            }
            (value, _) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Order {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(OrderVisitor)
    }
}

struct OrderVisitor;

impl<'de> Visitor<'de> for OrderVisitor {
    type Value = Order;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("JSON")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Order, A::Error> {
        let mut keys = Vec::new();
        while let Some(key) = map.next_key::<String>()? {
            keys.push((key, map.next_value::<Order>()?));
        }
        Ok(Order::Object(keys))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Order, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element::<Order>()? {
            items.push(item);
        }
        Ok(Order::Array(items))
    }

    fn visit_bool<E>(self, _: bool) -> Result<Order, E> {
        Ok(Order::Leaf)
    }

    fn visit_i64<E>(self, _: i64) -> Result<Order, E> {
        Ok(Order::Leaf)
    }

    fn visit_u64<E>(self, _: u64) -> Result<Order, E> {
        Ok(Order::Leaf)
    }

    fn visit_f64<E>(self, _: f64) -> Result<Order, E> {
        Ok(Order::Leaf)
    }

    fn visit_str<E>(self, _: &str) -> Result<Order, E> {
        Ok(Order::Leaf)
    }

    fn visit_unit<E>(self) -> Result<Order, E> {
        Ok(Order::Leaf)
    }

    fn visit_none<E>(self) -> Result<Order, E> {
        Ok(Order::Leaf)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Order, D::Error> {
        let _ = IgnoredAny::deserialize(deserializer)?;
        Ok(Order::Leaf)
    }
}
