//! This module provides an API to add custom methods to the datamodel.\
//! If the datamodel supports it, these Actions can be invoked from script and expressions.

#![allow(clippy::doc_lazy_continuation)]
#![allow(dead_code)]

use crate::datamodel::Data;
use crate::fsm::GlobalData;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// Trait to inject custom actions into the datamodel.
pub trait Action: Send {
    /// Executes the action.
    fn execute(&self, arguments: &[Data], global: &GlobalData) -> Result<Data, String>;

    /// Replacement for a generic "clone".
    fn get_copy(&self) -> Box<dyn Action>;
}

pub type ActionMap = HashMap<String, Box<dyn Action>>;

pub type ActionLock<'a> = MutexGuard<'a, ActionMap>;

/// Maintains am Arc to the map of actions.
#[derive(Default)]
pub struct ActionWrapper {
    pub actions: Arc<Mutex<ActionMap>>,
}

impl ActionWrapper {
    pub fn new() -> ActionWrapper {
        ActionWrapper {
            actions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn add_action(&mut self, name: &str, action: Box<dyn Action>) {
        self.actions
            .lock()
            .unwrap()
            .insert(name.to_string(), action);
    }

    pub fn get_copy(&self) -> ActionWrapper {
        ActionWrapper {
            actions: self.actions.clone(),
        }
    }

    pub fn get_map_copy(&self) -> ActionMap {
        let mut copy = HashMap::new();
        for (name, action) in self.lock().iter() {
            copy.insert(name.clone(), action.get_copy());
        }
        copy
    }

    pub fn lock(&self) -> ActionLock {
        self.actions.lock().unwrap()
    }

    pub fn execute(&self, action_name: &str, arguments: &[Data], global: &GlobalData) -> Result<Data, String> {
        let rt = if let Some(action) = self.actions.lock().unwrap().get_mut(action_name) {
            action.execute(arguments, global)
        } else {
            Err(format!("Action '{}' not found", action_name))
        };
        rt
    }
}
