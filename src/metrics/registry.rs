#![allow(dead_code)]

use std::collections::HashMap;

use super::{CollectorFactory, MetricCollector};

pub struct MetricRegistry {
    collectors: HashMap<String, Box<dyn MetricCollector>>,
}

impl MetricRegistry {
    pub fn new() -> Self {
        Self { collectors: HashMap::new() }
    }

    /// Register a collector.  Panics on duplicate name (safety-critical tool).
    pub fn register_boxed(&mut self, name: impl Into<String>, collector: Box<dyn MetricCollector>) {
        let name = name.into();
        assert!(!self.collectors.contains_key(&name), "duplicate collector registration: '{name}'");
        self.collectors.insert(name, collector);
    }

    pub fn get(&self, name: &str) -> Option<&dyn MetricCollector> {
        self.collectors.get(name).map(std::convert::AsRef::as_ref)
    }

    pub fn len(&self) -> usize {
        self.collectors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.collectors.is_empty()
    }
}

pub fn build_registry() -> MetricRegistry {
    let mut registry = MetricRegistry::new();
    for factory in inventory::iter::<CollectorFactory> {
        let instance = (factory.create)();
        debug_assert_eq!(
            instance.name(),
            factory.name,
            "collector name mismatch: factory says '{}' but instance says '{}'",
            factory.name,
            instance.name()
        );
        registry.register_boxed(factory.name, instance);
    }
    registry
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{MetricValue, Target};
    use anyhow::Result;

    struct DummyCollector {
        name: &'static str,
    }

    impl MetricCollector for DummyCollector {
        fn name(&self) -> &'static str {
            self.name
        }

        fn collect(&self, _target: &Target, _config: &serde_yaml::Value) -> Result<MetricValue> {
            Ok(MetricValue::new(0.0, ""))
        }
    }

    fn make(name: &'static str) -> Box<dyn MetricCollector> {
        Box::new(DummyCollector { name })
    }

    #[test]
    fn registry_get_returns_registered() {
        let mut registry = MetricRegistry::new();
        registry.register_boxed("foo", make("foo"));
        assert!(registry.get("foo").is_some());
    }

    #[test]
    fn registry_get_returns_none_for_unknown() {
        let registry = MetricRegistry::new();
        assert!(registry.get("unknown").is_none());
    }

    #[test]
    #[should_panic]
    fn duplicate_name_panics() {
        let mut registry = MetricRegistry::new();
        registry.register_boxed("foo", make("foo"));
        registry.register_boxed("foo", make("foo"));
    }

    #[test]
    fn registry_len_and_is_empty() {
        let mut registry = MetricRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        registry.register_boxed("a", make("a"));
        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn build_registry_returns_populated() {
        let registry = build_registry();
        assert!(!registry.is_empty());
    }
}
