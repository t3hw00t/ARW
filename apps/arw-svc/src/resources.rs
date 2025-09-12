use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone, Default)]
pub struct Resources(Arc<RwLock<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>>);

impl Resources {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn insert<T: Send + Sync + 'static>(&self, v: Arc<T>) {
        self.0.write().unwrap().insert(TypeId::of::<T>(), v);
    }
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.0
            .read()
            .unwrap()
            .get(&TypeId::of::<T>())
            .and_then(|a| a.clone().downcast::<T>().ok())
    }
}

pub mod governor_service;
pub mod hierarchy_service;
pub mod memory_service;
pub mod models_service;
pub mod cluster_service;
