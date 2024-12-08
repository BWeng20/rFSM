//! Implements the SCXML Data model for rFSM Expressions.\

use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use log::{debug, error};

use crate::datamodel::{Data, Datamodel, DatamodelFactory, GlobalDataArc, EVENT_VARIABLE_FIELD_DATA, EVENT_VARIABLE_FIELD_INVOKE_ID, EVENT_VARIABLE_FIELD_NAME, EVENT_VARIABLE_FIELD_ORIGIN, EVENT_VARIABLE_FIELD_ORIGIN_TYPE, EVENT_VARIABLE_FIELD_SEND_ID, EVENT_VARIABLE_FIELD_TYPE, EVENT_VARIABLE_NAME, DataArc, create_data_arc};
use crate::expression_engine::expressions::ExpressionResult;
use crate::expression_engine::expressions::ExpressionResult::Value;
use crate::expression_engine::parser::ExpressionParser;
use crate::fsm::{Event, ExecutableContentId, Fsm};

pub const RFSM_EXPRESSION_DATAMODEL: &str = "RFSM-EXPRESSION";
pub const RFSM_EXPRESSION_DATAMODEL_LC: &str = "rfsm-expression";

pub struct RFsmExpressionDatamodel {
    pub global_data: GlobalDataArc,
    pub readonly: HashSet<String>,
    null_data: Data,
}

impl RFsmExpressionDatamodel {
    pub fn new(global_data: GlobalDataArc) -> RFsmExpressionDatamodel {
        RFsmExpressionDatamodel {
            global_data,
            readonly: HashSet::new(),
            null_data: Data::Null(),
        }
    }

    fn set_arc(&mut self, name: &str, data: DataArc) {
        println!("set {} = {}", name, data.lock().unwrap());
        self.global_data
            .lock().unwrap()
            .data
            .set_undefined_arc(name.to_string(), data);
    }

    fn assign_internal(&mut self, left_expr: &str, right_expr: &str, allow_undefined: bool) -> bool {
        let exp = if allow_undefined {
            format!("{}?={}", left_expr, right_expr)
        } else {
            format!("{}={}", left_expr, right_expr)
        };
        println!("assign_internal {} ", exp);

        let ex = ExpressionParser::execute(exp, &mut self.global_data.lock().unwrap());
        let r = match ex {
            ExpressionResult::Value(_) => true,
            ExpressionResult::Error(error) => {
                // W3C says:\
                // If the location expression does not denote a valid location in the data model or
                // if the value specified (by 'expr' or children) is not a legal value for the
                // location specified, the SCXML Processor must place the error 'error.execution'
                // in the internal event queue.
                self.log(
                    format!(
                        "Could not assign {}={}, '{}'.",
                        left_expr, right_expr, error
                    )
                    .as_str(),
                );

                self.internal_error_execution();
                false
            }
        };
        r
    }

    fn execute_internal(&mut self, script: &Data, handle_error: bool) -> Result<DataArc, String> {
        // TODO
        println!("execute_internal {:?} ", script);

        if let Data::Source(source) = script {
            let result = ExpressionParser::execute(source.clone(), &mut self.global_data.lock().unwrap());
            match result {
                Value(val) => {
                    let value = val.lock().unwrap();
                    if let Data::Null() = value.deref() {
                        Ok(val.clone())
                    } else if let Data::Error(err) = value.deref() {
                        let msg = format!("Script Error: {} => {}", script, err);
                        error!("{}", msg);
                        if handle_error {
                            self.internal_error_execution();
                        }
                        Err(msg)
                    } else {
                        Ok(val.clone())
                    }
                }
                ExpressionResult::Error(e) => {
                    // Pretty print the error
                    let msg = format!("Script Error:  {} => {} ", script, e);
                    error!("{}", msg);
                    Err(msg)
                }
            }
        } else {
            Ok(create_data_arc(script.clone()))
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

    fn add_functions(&mut self, _fsm: &mut Fsm) {}

    fn set_from_state_data(&mut self, data: &HashMap<String, DataArc>, set_data: bool) {
        for (name, value) in data {
            if set_data {
                if let Data::Source(src) = value.lock().unwrap().deref() {
                    if !src.is_empty() {
                        // The data from state-data needs to be evaluated
                        // TODO: Escape
                        let data_lock =&mut self.global_data.lock().unwrap();
                        let rs = ExpressionParser::execute(src.clone(), data_lock);
                        match rs {
                            Value(val) => {
                                data_lock.data.set_undefined_arc(name.clone(), val.clone());
                            }
                            ExpressionResult::Error(err) => {
                                error!("Error on Initialize '{}': {}", name, err);
                                // W3C says:
                                // If the value specified for a <data> element (by 'src', children, or
                                // the environment) is not a legal data value, the SCXML Processor MUST
                                // raise place error.execution in the internal event queue and MUST
                                // create an empty data element in the data model with the specified id.
                                data_lock.data.set_undefined(name.clone(), Data::Null());
                                data_lock.enqueue_internal(Event::error_execution(&None, &None));
                            }
                        }
                    } else {
                        self.set(name, Data::Null());
                    }
                } else {
                    self.set_arc(name, value.clone());
                }
            }
        }
    }

    fn initialize_read_only_arc(&mut self, name: &str, value: DataArc) {
        // TODO
        self.set_arc(&name.to_string(), value);
    }

    fn set_arc(&mut self, name: &str, data: DataArc) {
        self.set_arc(&name.to_string(), data);
    }

    fn set_event(&mut self, event: &Event) {
        let data_value = match &event.param_values {
            None => match &event.content {
                None => create_data_arc(Data::Null()),
                Some(c) => c.clone(),
            },
            Some(pv) => {
                let mut data = HashMap::with_capacity(pv.len());
                for pair in pv.iter() {
                    data.insert(pair.name.clone(), pair.value.clone());
                }
                create_data_arc(Data::Map(data))
            }
        };

        let mut event_props = HashMap::with_capacity(7);

        event_props.insert(
            EVENT_VARIABLE_FIELD_NAME.to_string(),
            create_data_arc(Data::String(event.name.clone())),
        );
        event_props.insert(
            EVENT_VARIABLE_FIELD_TYPE.to_string(),
            create_data_arc(Data::String(event.etype.name().to_string())),
        );
        event_props.insert(
            EVENT_VARIABLE_FIELD_SEND_ID.to_string(),
            create_data_arc(option_to_data_value(&event.sendid)),
        );
        event_props.insert(
            EVENT_VARIABLE_FIELD_ORIGIN.to_string(),
            create_data_arc(option_to_data_value(&event.origin)),
        );
        event_props.insert(
            EVENT_VARIABLE_FIELD_ORIGIN_TYPE.to_string(),
            create_data_arc(option_to_data_value(&event.origin_type)),
        );
        event_props.insert(
            EVENT_VARIABLE_FIELD_INVOKE_ID.to_string(),
            create_data_arc(option_to_data_value(&event.invoke_id)),
        );
        event_props.insert(EVENT_VARIABLE_FIELD_DATA.to_string(), data_value);

        let mut ds = self.global_data.lock().unwrap();
        let event_name = EVENT_VARIABLE_NAME.to_string();
        // READONLY
        ds.data.set_undefined(event_name, Data::Map(event_props));
    }

    fn assign(&mut self, left_expr: &str, right_expr: &str) -> bool {
        self.assign_internal(left_expr, right_expr, false)
    }

    fn get_by_location(&mut self, location: &str) -> Result<DataArc, String> {
        match self.execute_internal(&Data::Source(location.to_string()), false) {
            Err(msg) => {
                self.internal_error_execution();
                Err(msg)
            }
            Ok(val) => Ok(val),
        }
    }

    fn clear(&mut self) {}

    fn execute(&mut self, script: &Data) -> Result<DataArc, String> {
        match self.execute_internal(script, true) {
            Ok(r) => match r.lock().unwrap().deref() {
                Data::Double(_)
                | Data::Source(_)
                | Data::String(_)
                | Data::Boolean(_)
                | Data::Null()
                | Data::None()
                | Data::Integer(_) => Ok(r.clone()),
                Data::Array(_) => Err("Illegal Result: Can't return array".to_string()),
                Data::Map(_) => Err("Illegal Result: Can't return maps".to_string()),
                Data::Error(err) => Err(err.clone()),
            },
            Err(err) => Err(err),
        }
    }

    fn execute_for_each(
        &mut self,
        array_expression: &str,
        item_name: &str,
        index: &str,
        execute_body: &mut dyn FnMut(&mut dyn Datamodel) -> bool,
    ) -> bool {
        #[cfg(feature = "Debug")]
        debug!("ForEach: array: {}", array_expression);
        let data = ExpressionParser::execute(array_expression.to_string(), &mut self.global_data.lock().unwrap());
        match data {
            Value(r) => {
                match r.lock().unwrap().deref() {
                    Data::Map(map) => {
                        let mut idx: i64 = 0;
                        if self.assign_internal(item_name, "null", true) {
                            for (name, item_value) in map {
                                #[cfg(feature = "Debug")]
                                debug!("ForEach: #{} {} {}={:?}", idx, name, item_name, item_value);
                                self.set_arc(item_name, item_value.clone());
                                if !index.is_empty() {
                                    self.set(index, Data::Integer(idx));
                                }
                                if !execute_body(self) {
                                    return false;
                                }
                                idx += 1;
                            }
                        }
                    }
                    Data::Array(array) => {
                        let mut idx: i64 = 0;
                        if self.assign_internal(item_name, "null", true) {
                            for data in array {
                                #[cfg(feature = "Debug")]
                                debug!("ForEach: #{} {:?}", idx, data);
                                self.set_arc(item_name, data.clone() );
                                if !index.is_empty() {
                                    self.set(index, Data::Integer(idx));
                                }
                                if !execute_body(self) {
                                    return false;
                                }
                                idx += 1;
                            }
                        }
                    }
                    _ => {
                        self.log("Resulting value is not a supported collection.");
                        self.internal_error_execution();
                    }
                }
                true
            }
            ExpressionResult::Error(e) => {
                self.log(&e.to_string());
                false
            }
        }
    }

    fn execute_condition(&mut self, script: &Data) -> Result<bool, String> {
        // W3C:
        // B.2.3 Conditional Expressions
        //   The Processor must convert ECMAScript expressions used in conditional expressions into their effective boolean
        //   value using the ToBoolean operator as described in Section 9.2 of [ECMASCRIPT-262].
        // EMCA says:
        //  1. If argument is a Boolean, return argument.
        //  2. If argument is one of undefined, null, +0𝔽, -0𝔽, NaN, 0ℤ, or the empty String, return false.
        //  3. If argument is an Object and argument has an [[IsHTMLDDA]] internal slot, return false.
        //     Remark: we have no such thing here.
        //  4. Return true.
        let r = match self.execute_internal(script, false) {
            Ok(val) => match val.arc.lock().unwrap().deref() {
                Data::Integer(v) => Ok(!(v != v || v.abs() == 0)),
                Data::Double(v) => Ok(!(v != v || v.abs() == 0f64)),
                Data::Source(s) | Data::String(s) => Ok(!s.is_empty()),
                Data::Boolean(b) => Ok(*b),
                Data::Array(_) => Ok(true),
                Data::Map(_) => Ok(true),
                Data::Null() => Ok(false),
                Data::None() => Ok(false),
                Data::Error(error) => Err(error.clone()),
            },
            Err(msg) => Err(msg),
        };
        println!("execute_condition {} => {:?}", script, r);
        r
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
