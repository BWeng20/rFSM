//! Implements the SCXML Data model for rFSM Expressions.

use crate::actions::{Action, ActionWrapper};
use std::collections::HashMap;
use std::ops::Deref;

use crate::common::{error, info};

#[cfg(feature = "Debug")]
use crate::common::debug;

use crate::datamodel::{
    create_data_arc, data_to_string, str_to_source, Data, DataArc, Datamodel, DatamodelFactory, GlobalDataArc,
    SourceCode, EVENT_VARIABLE_FIELD_DATA, EVENT_VARIABLE_FIELD_INVOKE_ID, EVENT_VARIABLE_FIELD_NAME,
    EVENT_VARIABLE_FIELD_ORIGIN, EVENT_VARIABLE_FIELD_ORIGIN_TYPE, EVENT_VARIABLE_FIELD_SEND_ID,
    EVENT_VARIABLE_FIELD_TYPE, EVENT_VARIABLE_NAME,
};
use crate::event_io_processor::SYS_IO_PROCESSORS;
use crate::expression_engine::expressions::{
    Expression, ExpressionAssign, ExpressionAssignUndefined, ExpressionConstant,
};
use crate::expression_engine::parser::ExpressionParser;
use crate::fsm::{Event, ExecutableContentId, Fsm, GlobalData, StateId};

pub const RFSM_EXPRESSION_DATAMODEL: &str = "RFSM-EXPRESSION";
pub const RFSM_EXPRESSION_DATAMODEL_LC: &str = "rfsm-expression";

pub struct RFsmExpressionDatamodel {
    pub global_data: GlobalDataArc,
    null_data: DataArc,
    compilations: HashMap<usize, Box<dyn Expression>>,
}

impl RFsmExpressionDatamodel {
    pub fn new(global_data: GlobalDataArc) -> RFsmExpressionDatamodel {
        RFsmExpressionDatamodel {
            global_data,
            null_data: create_data_arc(Data::Null()),
            compilations: HashMap::new(),
        }
    }

    fn compile(&mut self, source: &SourceCode) -> Result<Box<dyn Expression>, String> {
        if source.source_id == 0 {
            ExpressionParser::parse(source.source.clone())
        } else {
            let compiled = self.compilations.get(&source.source_id);
            match compiled {
                None => {
                    let expression = ExpressionParser::parse(source.source.clone())?;
                    self.compilations
                        .insert(source.source_id, expression.get_copy());
                    Ok(expression)
                }
                Some(expression) => {
                    #[cfg(feature = "Debug")]
                    debug!(
                        "get expression from cache : #{} '{}' -> {:?} ",
                        source.source_id, source.source, expression
                    );
                    Ok(expression.get_copy())
                }
            }
        }
    }

    fn parse(&mut self, data: &Data) -> Result<Box<dyn Expression>, String> {
        match data {
            Data::String(_)
            | Data::Boolean(_)
            | Data::Array(_)
            | Data::Map(_)
            | Data::Null()
            | Data::Integer(_)
            | Data::None()
            | Data::Double(_) => Ok(Box::new(ExpressionConstant::new(data.clone()))),
            Data::Error(err) => Err(err.clone()),
            Data::Source(source) => self.compile(source),
        }
    }

    fn assign_internal(&mut self, left_expr: &Data, right_expr: &Data, allow_undefined: bool) -> bool {
        let r = match (self.parse(left_expr), self.parse(right_expr)) {
            (Ok(left_parsed), Ok(right_parsed)) => {
                let expression: Box<dyn Expression> = if allow_undefined {
                    Box::new(ExpressionAssignUndefined::new(left_parsed, right_parsed))
                } else {
                    Box::new(ExpressionAssign::new(left_parsed, right_parsed))
                };
                #[cfg(feature = "Debug")]
                debug!("assign_internal: {:?} ", expression);
                let ex = expression.execute(&mut self.global_data.lock().unwrap(), allow_undefined);
                let r = match ex {
                    Ok(_) => true,
                    Err(error) => {
                        self.log(format!("Can't assign {:?}: {}.", expression, error).as_str());
                        false
                    }
                };
                r
            }
            (Err(e1), _) => {
                self.log(format!("Can't assign {}={}: {}", left_expr, right_expr, e1).as_str());
                false
            }
            (_, Err(e2)) => {
                self.log(format!("Can't assign {}={}: {}", left_expr, right_expr, e2).as_str());
                false
            }
        };
        if !r {
            // W3C says:\
            // If the location expression does not denote a valid location in the data model or
            // if the value specified (by 'expr' or children) is not a legal value for the
            // location specified, the SCXML Processor must place the error 'error.execution'
            // in the internal event queue.
            self.internal_error_execution();
        }
        r
    }

    fn execute_internal_source(&mut self, source: &SourceCode, handle_error: bool) -> Result<DataArc, String> {
        let parser_result = self.compile(source);
        match parser_result {
            Ok(expression) => {
                let result = expression.execute(&mut self.global_data.lock().unwrap(), false);
                match result {
                    Ok(val) => {
                        let value = val.lock().unwrap();
                        if let Data::Null() = value.deref() {
                            Ok(val.clone())
                        } else if let Data::Error(err) = value.deref() {
                            let msg = format!("Script Error: {} => {}", source, err);
                            error!("{}", msg);
                            if handle_error {
                                self.internal_error_execution();
                            }
                            Err(msg)
                        } else {
                            Ok(val.clone())
                        }
                    }
                    Err(e) => {
                        // Pretty print the error
                        let msg = format!("Script Error:  {} => {} ", source, e);
                        error!("{}", msg);
                        Err(msg)
                    }
                }
            }
            Err(err) => Err(err),
        }
    }

    fn execute_internal(&mut self, script: &Data, handle_error: bool) -> Result<DataArc, String> {
        if let Data::Source(source) = script {
            self.execute_internal_source(source, handle_error)
        } else {
            Ok(create_data_arc(script.clone()))
        }
    }

    pub fn add_internal_functions_to_wrapper(actions: &mut ActionWrapper) {
        actions.add_action("indexOf", Box::new(IndexOfAction {}));
        actions.add_action("length", Box::new(LengthAction {}));
        actions.add_action("isDefined", Box::new(IsDefinedAction {}));
        actions.add_action("abs", Box::new(AbsAction {}));
        actions.add_action("toString", Box::new(ToStringAction {}));
        actions.add_action("log", Box::new(LogAction {}));
    }

    pub fn add_internal_fsm_functions(&mut self, fsm: &mut Fsm) {
        let mut guard = self.global_data.lock().unwrap();
        Self::add_internal_functions_to_wrapper(&mut guard.actions);
        guard.actions.add_action("In", Box::new(InAction::new(fsm)));
    }

    fn resolve_source_data(&mut self, data: &Data) -> Result<DataArc, String> {
        if let Data::Source(_) = &data {
            self.execute_internal(data, false)
        } else {
            Ok(create_data_arc(data.clone()))
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

/// Action to implement the mandatory SCXML-Datamodel function "In".
#[derive(Clone)]
pub struct InAction {
    pub state_name_to_id: HashMap<String, StateId>,
}

impl InAction {
    pub fn new(fsm: &mut Fsm) -> InAction {
        let mut state_name_to_id = HashMap::new();
        for state in fsm.states.as_slice() {
            state_name_to_id.insert(state.name.clone(), state.id);
        }

        InAction { state_name_to_id }
    }
}

impl Action for InAction {
    fn execute(&self, arguments: &[Data], global: &GlobalData) -> Result<Data, String> {
        if arguments.len() == 1 {
            match &arguments[0] {
                Data::String(state_name) => {
                    let r = match self.state_name_to_id.get(state_name) {
                        None => false,
                        Some(state_id) => global.configuration.data.contains(state_id),
                    };
                    #[cfg(feature = "Debug")]
                    debug!("In('{}') -> {}", state_name, r);
                    Ok(Data::Boolean(r))
                }
                _ => Err("Illegal argument type for 'In'".to_string()),
            }
        } else {
            Err("Wrong arguments for 'In'.".to_string())
        }
    }

    fn get_copy(&self) -> Box<dyn Action> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
pub struct ToStringAction {}

impl Action for ToStringAction {
    fn execute(&self, arguments: &[Data], _global: &GlobalData) -> Result<Data, String> {
        if arguments.len() == 1 {
            match data_to_string(&arguments[0]) {
                Ok(s) => Ok(Data::String(s)),
                Err(err) => Err(err),
            }
        } else {
            Err("Wrong number of arguments for 'toString'.".to_string())
        }
    }

    fn get_copy(&self) -> Box<dyn Action> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
pub struct IndexOfAction {}

impl Action for IndexOfAction {
    fn execute(&self, arguments: &[Data], _global: &GlobalData) -> Result<Data, String> {
        if arguments.len() == 2 {
            match (&arguments[0], &arguments[1]) {
                (Data::String(s1), Data::String(s2)) => {
                    let r = match s1.find(s2) {
                        None => -1,
                        Some(idx) => idx as i64,
                    };
                    #[cfg(feature = "Debug")]
                    debug!("indexOf({},{}) -> {}", s1, s2, r);
                    Ok(Data::Integer(r))
                }
                (_, _) => Err("Illegal argument types for 'indexOf'".to_string()),
            }
        } else {
            Err("Wrong arguments for 'indexOf'.".to_string())
        }
    }

    fn get_copy(&self) -> Box<dyn Action> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
pub struct LengthAction {}
impl Action for LengthAction {
    fn execute(&self, arguments: &[Data], _global: &GlobalData) -> Result<Data, String> {
        if arguments.len() == 1 {
            let r = match &arguments[0] {
                Data::String(s) => s.len(),
                Data::Array(a) => a.len(),
                Data::Map(m) => m.len(),
                Data::Source(s) => s.len(),
                _ => {
                    return Err("Wrong argument type for 'length'.".to_string());
                }
            };
            Ok(Data::Integer(r as i64))
        } else {
            Err("Wrong number of arguments for 'length'.".to_string())
        }
    }

    fn get_copy(&self) -> Box<dyn Action> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
pub struct AbsAction {}
impl Action for AbsAction {
    fn execute(&self, arguments: &[Data], _global: &GlobalData) -> Result<Data, String> {
        if arguments.len() == 1 {
            match &arguments[0] {
                Data::Integer(value) => Ok(Data::Integer(value.abs())),
                Data::Double(value) => Ok(Data::Double(value.abs())),
                _ => Err("Wrong argument type for 'abs'.".to_string()),
            }
        } else {
            Err("Wrong number of arguments for 'abs'.".to_string())
        }
    }

    fn get_copy(&self) -> Box<dyn Action> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
pub struct IsDefinedAction {}
impl Action for IsDefinedAction {
    fn execute(&self, arguments: &[Data], _global: &GlobalData) -> Result<Data, String> {
        if arguments.len() == 1 {
            match &arguments[0] {
                Data::Error(_) | Data::None() => Ok(Data::Boolean(false)),
                _ => Ok(Data::Boolean(true)),
            }
        } else {
            Err("Wrong number of arguments for 'isDefined'.".to_string())
        }
    }

    fn get_copy(&self) -> Box<dyn Action> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
pub struct LogAction {}
impl Action for LogAction {
    fn execute(&self, arguments: &[Data], _global: &GlobalData) -> Result<Data, String> {
        if arguments.len() == 1 {
            match data_to_string(&arguments[0]) {
                Ok(message) => {
                    info!("{}", message);
                    Ok(Data::None())
                }
                Err(err) => Err(err),
            }
        } else {
            Err("Wrong number of arguments for 'log'.".to_string())
        }
    }

    fn get_copy(&self) -> Box<dyn Action> {
        Box::new(self.clone())
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

    fn add_functions(&mut self, fsm: &mut Fsm) {
        self.add_internal_fsm_functions(fsm);
    }

    fn set_ioprocessors(&mut self) {
        let session_id = self.global_s().lock().unwrap().session_id;
        let mut io_processors_dings = HashMap::new();
        for (name, processor) in &self.global_data.lock().unwrap().io_processors {
            let mut processor_data = HashMap::new();
            let location = create_data_arc(Data::String(
                processor.lock().unwrap().get_location(session_id),
            ));
            processor_data.insert("location".to_string(), location);
            io_processors_dings.insert(name.clone(), create_data_arc(Data::Map(processor_data)));
        }
        let mut data_arc = create_data_arc(Data::Map(io_processors_dings));
        data_arc.set_readonly(true);
        self.set_arc(SYS_IO_PROCESSORS, data_arc, true);
    }

    fn set_from_state_data(&mut self, data: &HashMap<String, DataArc>, set_data: bool) {
        for (name, value) in data {
            if set_data {
                if let Data::Source(src) = value.lock().unwrap().deref() {
                    if !src.is_empty() {
                        // The data from state-data needs to be evaluated
                        let rs = self.execute_internal_source(src, false);
                        let data_lock = &mut self.global_data.lock().unwrap();
                        match rs {
                            Ok(val) => {
                                data_lock.data.set_undefined_arc(name.clone(), val.clone());
                            }
                            Err(err) => {
                                error!("Error on Initialize '{}': {}", name, err);
                                // W3C says:
                                // If the value specified for a <data> element (by 'src', children, or
                                // the environment) is not a legal data value, the SCXML Processor MUST
                                // raise place error.execution in the internal event queue and MUST
                                // create an empty data element in the data model with the specified id.
                                data_lock.data.set_undefined(name.clone(), Data::None());
                                data_lock.enqueue_internal(Event::error_execution(&None, &None));
                            }
                        }
                    } else {
                        self.set(name, Data::Null(), true);
                    }
                } else {
                    self.set_arc(name, value.clone(), true);
                }
            } else {
                self.set(name, Data::None(), true);
            }
        }
    }

    fn initialize_read_only_arc(&mut self, name: &str, mut value: DataArc) {
        value.set_readonly(true);
        self.global_data
            .lock()
            .unwrap()
            .data
            .set_undefined_arc(name.to_string(), value);
    }

    fn set_arc(&mut self, name: &str, data: DataArc, allow_undefined: bool) {
        if allow_undefined {
            self.global_data
                .lock()
                .unwrap()
                .data
                .set_undefined_arc(name.to_string(), data);
        } else {
            self.global_data
                .lock()
                .unwrap()
                .data
                .set_arc(name.to_string(), data);
        }
    }

    fn set_event(&mut self, event: &Event) {
        let data_value = match &event.param_values {
            None => match &event.content {
                None => self.null_data.clone(),
                Some(cd) => match self.resolve_source_data(cd) {
                    Ok(val) => val,
                    Err(err) => {
                        error!("Can't eval event content '{}': {}", cd, err);
                        self.null_data.clone()
                    }
                },
            },
            Some(pv) => {
                let mut data = HashMap::with_capacity(pv.len());
                for pair in pv.iter() {
                    match self.resolve_source_data(&pair.value) {
                        Ok(val) => {
                            data.insert(pair.name.clone(), val);
                        }
                        Err(err) => {
                            error!(
                                "Can set event data '{} = {}': {}",
                                pair.name, pair.value, err
                            )
                        }
                    }
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
        let mut event_arc = create_data_arc(Data::Map(event_props));
        event_arc.set_readonly(true);
        ds.data.map.remove(&event_name);
        ds.data.set_undefined_arc(event_name, event_arc);
    }

    fn assign(&mut self, left_expr: &Data, right_expr: &Data) -> bool {
        self.assign_internal(left_expr, right_expr, false)
    }

    fn get_by_location(&mut self, location: &str) -> Result<DataArc, String> {
        match self.execute_internal(&str_to_source(location), false) {
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
            Ok(r) => {
                match r.lock().unwrap().deref() {
                    Data::Double(_)
                    | Data::Source(_)
                    | Data::String(_)
                    | Data::Boolean(_)
                    | Data::Null()
                    | Data::None()
                    | Data::Integer(_) => (),
                    Data::Array(_) => return Err("Illegal Result: Can't return array".to_string()),
                    Data::Map(_) => return Err("Illegal Result: Can't return maps".to_string()),
                    Data::Error(err) => return Err(err.clone()),
                }
                Ok(r)
            }
            Err(err) => Err(err),
        }
    }

    fn execute_for_each(
        &mut self,
        array_expression: &Data,
        item_name: &str,
        index: &str,
        execute_body: &mut dyn FnMut(&mut dyn Datamodel) -> bool,
    ) -> bool {
        #[cfg(feature = "Debug")]
        debug!("ForEach: array: {}", array_expression);
        let data = self.execute_internal(array_expression, false);
        match data {
            Ok(r) => {
                let dc = r.lock().unwrap().clone();
                match dc {
                    Data::Map(map) => {
                        let mut idx: i64 = 0;
                        if self.assign_internal(&str_to_source(item_name), &Data::Null(), true) {
                            #[allow(unused_variables)]
                            for (name, item_value) in map {
                                #[cfg(feature = "Debug")]
                                debug!("ForEach: #{} {} {}={}", idx, name, item_name, item_value);
                                self.set_arc(item_name, item_value.clone(), true);
                                if !index.is_empty() {
                                    self.set(index, Data::Integer(idx), true);
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
                        if self.assign_internal(&str_to_source(item_name), &Data::Null(), true) {
                            for data in array {
                                #[cfg(feature = "Debug")]
                                debug!("ForEach: #{} {:?}", idx, data);
                                self.set_arc(item_name, data.clone(), true);
                                if !index.is_empty() {
                                    self.set(index, Data::Integer(idx), true);
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
            Err(e) => {
                self.log(&e.to_string());
                false
            }
        }
    }

    #[allow(clippy::eq_op)] // For NaN test, as "is_nan" method is not yet stable.
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
                Data::Integer(v) => {
                    // NaN Test
                    Ok(!(v != v || v.abs() == 0))
                }
                Data::Double(v) => Ok(!(v != v || v.abs() == 0f64)),
                Data::Source(s) => Ok(!s.is_empty()),
                Data::String(s) => Ok(!s.is_empty()),
                Data::Boolean(b) => Ok(*b),
                Data::Array(_) => Ok(true),
                Data::Map(_) => Ok(true),
                Data::Null() => Ok(false),
                Data::None() => Ok(false),
                Data::Error(error) => Err(error.clone()),
            },
            Err(msg) => Err(msg),
        };
        #[cfg(feature = "Debug")]
        debug!("execute_condition: {} => {:?}", script, r);
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

#[cfg(test)]
mod tests {
    use crate::common::init_logging;
    use crate::datamodel::expression_engine::RFsmExpressionDatamodel;
    use crate::datamodel::{create_data_arc, create_global_data_arc, Data};
    use crate::expression_engine::expressions::ExpressionResult;
    use crate::expression_engine::parser::ExpressionParser;
    use std::collections::HashMap;

    #[test]
    fn index_of_works() {
        init_logging();
        let gd = create_global_data_arc();
        RFsmExpressionDatamodel::add_internal_functions_to_wrapper(&mut gd.lock().unwrap().actions);

        // As normal function.
        let rs = ExpressionParser::execute("indexOf('abc', 'bc')".to_string(), &mut gd.lock().unwrap());

        assert_eq!(rs, Ok(create_data_arc(Data::Integer(1i64))));

        // As Member function.
        let rs = ExpressionParser::execute("'abc'.indexOf('bc')".to_string(), &mut gd.lock().unwrap());

        assert_eq!(rs, Ok(create_data_arc(Data::Integer(1i64))));

        println!("{:?}", rs);
    }

    #[test]
    fn length_works() {
        init_logging();
        let gd = create_global_data_arc();
        RFsmExpressionDatamodel::add_internal_functions_to_wrapper(&mut gd.lock().unwrap().actions);

        // As normal function.
        // On text
        let rs = ExpressionParser::execute("length('abc')".to_string(), &mut gd.lock().unwrap());
        assert_eq!(
            rs,
            ExpressionResult::Ok(create_data_arc(Data::Integer(3i64)))
        );

        // On an array
        let rs = ExpressionParser::execute("length([1,2,3,4])".to_string(), &mut gd.lock().unwrap());
        assert_eq!(
            rs,
            ExpressionResult::Ok(create_data_arc(Data::Integer(4i64)))
        );
        // On a map
        let mut m = HashMap::new();
        m.insert("a".to_string(), create_data_arc(Data::Integer(1i64)));
        m.insert("b".to_string(), create_data_arc(Data::Integer(5i64)));
        m.insert("c".to_string(), create_data_arc(Data::Integer(4i64)));
        m.insert("d".to_string(), create_data_arc(Data::Integer(3i64)));
        m.insert("e".to_string(), create_data_arc(Data::Integer(2i64)));
        gd.lock()
            .unwrap()
            .data
            .map
            .insert("v1".to_string(), create_data_arc(Data::Map(m)));
        let rs = ExpressionParser::execute("v1.length()".to_string(), &mut gd.lock().unwrap());
        assert_eq!(
            rs,
            ExpressionResult::Ok(create_data_arc(Data::Integer(5i64)))
        );

        // As Member function.
        let rs = ExpressionParser::execute("'abc'.length()".to_string(), &mut gd.lock().unwrap());
        assert_eq!(
            rs,
            ExpressionResult::Ok(create_data_arc(Data::Integer(3i64)))
        );

        println!("{:?}", rs);
    }

    #[test]
    fn abs_of_works() {
        init_logging();
        let gd = create_global_data_arc();
        RFsmExpressionDatamodel::add_internal_functions_to_wrapper(&mut gd.lock().unwrap().actions);

        // As normal function.
        let rs = ExpressionParser::execute("abs(-102.111)".to_string(), &mut gd.lock().unwrap());

        assert_eq!(rs, Ok(create_data_arc(Data::Double(102.111))));

        // As Member function.
        let rs = ExpressionParser::execute_str("abs(-124)", &mut gd.lock().unwrap());

        assert_eq!(rs, Ok(create_data_arc(Data::Integer(124))));
    }

    #[test]
    fn to_string_works() {
        init_logging();
        let gd = create_global_data_arc();
        RFsmExpressionDatamodel::add_internal_functions_to_wrapper(&mut gd.lock().unwrap().actions);

        let rs = ExpressionParser::execute("toString(-102)".to_string(), &mut gd.lock().unwrap());
        assert_eq!(rs, Ok(create_data_arc(Data::String("-102".to_string()))));

        let rs = ExpressionParser::execute(
            "[1,2,'abc',[1.2]].toString()".to_string(),
            &mut gd.lock().unwrap(),
        );
        assert_eq!(
            rs,
            Ok(create_data_arc(Data::String("1,2,abc,1.2".to_string())))
        );

        let rs = ExpressionParser::execute("'abcdef'.toString()".to_string(), &mut gd.lock().unwrap());
        assert_eq!(rs, Ok(create_data_arc(Data::String("abcdef".to_string()))));
    }
}
