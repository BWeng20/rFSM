//! Implements the SCXML Data model for rFSM Expressions.\

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use log::error;

use crate::datamodel::{
    Data, Datamodel, DatamodelFactory, DataStore, EVENT_VARIABLE_FIELD_DATA, EVENT_VARIABLE_FIELD_INVOKE_ID,
    EVENT_VARIABLE_FIELD_NAME, EVENT_VARIABLE_FIELD_ORIGIN, EVENT_VARIABLE_FIELD_ORIGIN_TYPE,
    EVENT_VARIABLE_FIELD_SEND_ID, EVENT_VARIABLE_FIELD_TYPE, EVENT_VARIABLE_NAME, GlobalDataArc,
};
use crate::event_io_processor::EventIOProcessor;
use crate::expression_engine::expressions::Context;
use crate::expression_engine::parser::ExpressionParser;
use crate::fsm::{Event, ExecutableContentId, Fsm};

pub const RFSM_EXPRESSION_DATAMODEL: &str = "RFSM-EXPRESSION";
pub const RFSM_EXPRESSION_DATAMODEL_LC: &str = "rfsm-expression";

pub struct RFsmExpressionDatamodel {
    pub data: DataStore,
    pub global_data: GlobalDataArc,
}

impl RFsmExpressionDatamodel {
    pub fn new(global_data: GlobalDataArc) -> RFsmExpressionDatamodel {
        RFsmExpressionDatamodel {
            data: DataStore::new(),
            global_data,
        }
    }

    fn assign_internal(&mut self, _left_expr: &str, _right_expr: &str, _allow_undefined: bool) -> bool {
        todo!()
    }

    fn execute_internal(&mut self, script: &str, handle_error: bool) -> Result<Data, String> {
        // TODO
        let mut ctx = Context::new();
        let result = ExpressionParser::execute(script.to_string(), &mut ctx);
        match result {
            Ok(res) => {
                if let Data::Null() = res {
                    #[cfg(feature = "Debug")]
                    debug!("Execute: {} => undefined", script);
                    Ok(res)
                } else if let Data::Error(err) = res {
                    let msg = format!("Script Error: {} => {}", script, err);
                    error!("{}", msg);
                    if handle_error {
                        self.internal_error_execution();
                    }
                    Err(msg)
                } else {
                    Ok(res)
                }
            }
            Err(e) => {
                // Pretty print the error
                let msg = format!("Script Error:  {} => {} ", script, e);
                error!("{}", msg);
                Err(msg)
            }
        }
    }
}

pub struct RFsmExpressionDatamodelFactory {}

impl DatamodelFactory for RFsmExpressionDatamodelFactory {
    fn create(&mut self, global_data: GlobalDataArc, _options: &HashMap<String, String>) -> Box<dyn Datamodel> {
        Box::new(RFsmExpressionDatamodel::new(global_data))
    }
}

fn option_to_data_value(val: &Option<String>) -> Data {
    match val {
        Some(s) => Data::String(s.clone()),
        None => Data::Null(),
    }
}

impl Datamodel for RFsmExpressionDatamodel {
    fn global(&mut self) -> &mut GlobalDataArc {
        &mut self.global_data
    }
    fn global_s(&self) -> &GlobalDataArc {
        &self.global_data
    }

    fn get_name(&self) -> &str {
        RFSM_EXPRESSION_DATAMODEL
    }

    fn set_from_data_store(&mut self, data: &DataStore, set_data: bool) {
        for (name, data) in &data.values {
            if set_data {
                if let Data::String(dv) = data {
                    // TODO
                    let mut ctx = Context::new();

                    let rs = ExpressionParser::execute(dv.clone(), &mut ctx);
                    match rs {
                        Ok(val) => {
                            self.data.set(name.as_str(), val);
                        }
                        Err(err) => {
                            error!("Error on Initialize '{}': {}", name, err);
                            // W3C says:
                            // If the value specified for a <data> element (by 'src', children, or
                            // the environment) is not a legal data value, the SCXML Processor MUST
                            // raise place error.execution in the internal event queue and MUST
                            // create an empty data element in the data model with the specified id.
                            self.data.set(name.as_str(), Data::Null());
                            self.internal_error_execution();
                        }
                    }
                } else {
                    self.data.set(name.as_str(), data.clone());
                }
            } else {
                self.data.set(name.as_str(), Data::Null());
            }
        }
    }

    fn add_functions(&mut self, _fsm: &mut Fsm) {}

    fn initialize_read_only(&mut self, name: &str, value: &str) {
        // TODO
        self.data.set(name, Data::String(value.to_string()));
    }

    fn set(&mut self, name: &str, data: Data) {
        self.data.set(name, data);
    }

    fn set_event(&mut self, event: &Event) {
        let data_value = match &event.param_values {
            None => match &event.content {
                None => Data::Null(),
                Some(c) => Data::String(c.clone()),
            },
            Some(pv) => {
                let mut data = HashMap::with_capacity(pv.len());
                for pair in pv.iter() {
                    data.insert(pair.name.clone(), pair.value.clone());
                }
                Data::Map(data)
            }
        };

        let mut event_props = HashMap::with_capacity(7);

        event_props.insert(
            EVENT_VARIABLE_FIELD_NAME.to_string(),
            Data::String(event.name.clone()),
        );
        event_props.insert(
            EVENT_VARIABLE_FIELD_TYPE.to_string(),
            Data::String(event.etype.name().to_string()),
        );
        event_props.insert(
            EVENT_VARIABLE_FIELD_SEND_ID.to_string(),
            option_to_data_value(&event.sendid),
        );
        event_props.insert(
            EVENT_VARIABLE_FIELD_ORIGIN.to_string(),
            option_to_data_value(&event.origin),
        );
        event_props.insert(
            EVENT_VARIABLE_FIELD_ORIGIN_TYPE.to_string(),
            option_to_data_value(&event.origin_type),
        );
        event_props.insert(
            EVENT_VARIABLE_FIELD_INVOKE_ID.to_string(),
            option_to_data_value(&event.invoke_id),
        );
        event_props.insert(EVENT_VARIABLE_FIELD_DATA.to_string(), data_value);

        self.data.set(EVENT_VARIABLE_NAME, Data::Map(event_props));
    }

    fn assign(&mut self, left_expr: &str, right_expr: &str) -> bool {
        self.assign_internal(left_expr, right_expr, false)
    }

    fn get_by_location(&mut self, _location: &str) -> Result<Data, String> {
        todo!()
    }

    fn get_io_processor(&mut self, _name: &str) -> Option<Arc<Mutex<Box<dyn EventIOProcessor>>>> {
        todo!()
    }

    fn send(&mut self, _ioc_processor: &str, _target: &str, _event: Event) -> bool {
        todo!()
    }

    fn get_mut(&mut self, _name: &str) -> Option<&mut Data> {
        todo!()
    }

    fn clear(&mut self) {}

    fn execute(&mut self, script: &str) -> Result<String, String> {
        let r = self.execute_internal(script, true)?;
        Ok(r.to_string())
    }

    fn execute_for_each(
        &mut self,
        _array_expression: &str,
        _item: &str,
        _index: &str,
        _execute_body: &mut dyn FnMut(&mut dyn Datamodel) -> bool,
    ) -> bool {
        todo!()
    }

    fn execute_condition(&mut self, _script: &str) -> Result<bool, String> {
        todo!();
    }

    #[allow(non_snake_case)]
    fn executeContent(&mut self, fsm: &Fsm, content_id: ExecutableContentId) -> bool {
        let ec = fsm.executableContent.get(&content_id);
        for e in ec.unwrap().iter() {
            if !e.execute(self, fsm) {
                return false;
            }
        }
        true
    }
}
