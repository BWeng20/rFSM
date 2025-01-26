//! Defines the API used to access the data models.

use crate::common::{debug, error, info, warn};
use crate::expression_engine::lexer::{ExpressionLexer, Token};
use crate::fsm::{
    vec_to_string, CommonContent, Event, ExecutableContentId, Fsm, GlobalData, InvokeId, ParamPair, Parameter, State,
    StateId,
};
use std::any::Any;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::ops::Deref;
use std::sync::{Arc, LockResult, Mutex, MutexGuard};

use crate::actions::ActionMap;
use crate::event_io_processor::EventIOProcessor;

#[cfg(feature = "ECMAScriptModel")]
pub mod ecma_script;

#[cfg(feature = "RfsmExpressionModel")]
pub mod expression_engine;

/// Gets the global data store from datamodel.
macro_rules! get_global {
    ($x:expr) => {
        $x.global().lock().unwrap()
    };
}

pub const DATAMODEL_OPTION_PREFIX: &str = "datamodel:";

pub const NULL_DATAMODEL: &str = "NULL";
pub const NULL_DATAMODEL_LC: &str = "null";

pub const SCXML_INVOKE_TYPE: &str = "http://www.w3.org/TR/scxml";

/// W3C: Processors MAY define short form notations as an authoring convenience
/// (e.g., "scxml" as equivalent to <http://www.w3.org/TR/scxml> ).
pub const SCXML_INVOKE_TYPE_SHORT: &str = "scxml";

pub const SCXML_EVENT_PROCESSOR: &str = "http://www.w3.org/TR/scxml/#SCXMLEventProcessor";

#[cfg(feature = "BasicHttpEventIOProcessor")]
pub const BASIC_HTTP_EVENT_PROCESSOR: &str = "http://www.w3.org/TR/scxml/#BasicHTTPEventProcessor";

/// Name of system variable "_sessionid".\
/// *W3C says*:\
/// The SCXML Processor MUST bind the variable _sessionid at load time to the system-generated id
/// for the current SCXML session. (This is of type NMTOKEN.) The Processor MUST keep the variable
/// bound to this value until the session terminates.
pub const SESSION_ID_VARIABLE_NAME: &str = "_sessionid";

/// Name of system variable "_name".
/// *W3C says*:\
/// The SCXML Processor MUST bind the variable _name at load time to the value of the 'name'
/// attribute of the \<scxml\> element. The Processor MUST keep the variable bound to this
/// value until the session terminates.
pub const SESSION_NAME_VARIABLE_NAME: &str = "_name";

/// Name of system variable "_event" for events
pub const EVENT_VARIABLE_NAME: &str = "_event";

/// Name of field "name" of system variable "_event"
pub const EVENT_VARIABLE_FIELD_NAME: &str = "name";

/// Name of field "type" of system variable "_event"
pub const EVENT_VARIABLE_FIELD_TYPE: &str = "type";

/// Name of field of system variable "_event" "sendid"
pub const EVENT_VARIABLE_FIELD_SEND_ID: &str = "sendid";

/// Name of field "origin" of system variable "_event"
pub const EVENT_VARIABLE_FIELD_ORIGIN: &str = "origin";

/// Name of field "origintype" of system variable "_event"
pub const EVENT_VARIABLE_FIELD_ORIGIN_TYPE: &str = "origintype";

/// Name of field "invokeid" of system variable "_event"
pub const EVENT_VARIABLE_FIELD_INVOKE_ID: &str = "invokeid";

/// Name of field "data" of system variable "_event"
pub const EVENT_VARIABLE_FIELD_DATA: &str = "data";

/// Factory trait to handle creation of data-models dynamically.
pub trait DatamodelFactory: Send {
    /// Create a NEW datamodel.
    fn create(&mut self, global_data: GlobalDataArc, options: &HashMap<String, String>) -> Box<dyn Datamodel>;
}

pub type GlobalDataLock<'a> = MutexGuard<'a, GlobalData>;

/// Currently we assume that we need access to the global-data via a mutex as RUST doesn't allow access to it
/// from callbacks as used in ECMA-implementations and timers. If not, change this type to "GlobalData" and adapt implementation.
pub type GlobalDataArc = Arc<Mutex<GlobalData>>;

/// Helper to create the global data instance. Should be used to minimize dependencies.
pub fn create_global_data_arc() -> GlobalDataArc {
    GlobalDataArc::new(Mutex::from(crate::fsm::GlobalData::new()))
}

/// Data model interface trait.\
/// *W3C says*:\
/// The Data Model offers the capability of storing, reading, and modifying a set of data that is internal to the state machine.
/// This specification does not mandate any specific data model, but instead defines a set of abstract capabilities that can
/// be realized by various languages, such as ECMAScript or XML/XPath. Implementations may choose the set of data models that
/// they support. In addition to the underlying data structure, the data model defines a set of expressions as described in
/// 5.9 Expressions. These expressions are used to refer to specific locations in the data model, to compute values to
/// assign to those locations, and to evaluate boolean conditions.\
/// Finally, the data model includes a set of system variables, as defined in 5.10 System Variables, which are automatically maintained
/// by the SCXML processor.
pub trait Datamodel {
    /// Returns the global data.\
    /// As the data model needs access to other global variables and rust doesn't like
    /// accessing data of parents (Fsm in this case) from inside a child (the actual Datamodel), most global data is
    /// store in the "GlobalData" struct that is owned by the data model.
    fn global(&mut self) -> &mut GlobalDataArc;

    fn global_s(&self) -> &GlobalDataArc;

    /// Get the name of the data model as defined by the \<scxml\> attribute "datamodel".
    fn get_name(&self) -> &str;

    /// Adds the "In" and other function.\
    /// If needed, adds also "log" function.
    fn add_functions(&mut self, fsm: &mut Fsm);

    /// sets '_ioprocessors'.
    fn set_ioprocessors(&mut self);

    /// Initialize the data model for one data-store.
    /// This method is called for the global data and for the data of each state.
    #[allow(non_snake_case)]
    fn initializeDataModel(&mut self, fsm: &mut Fsm, state: StateId, set_data: bool) {
        let state_obj: &State = fsm.get_state_by_id_mut(state);
        // Set all (simple) global variables.
        self.set_from_state_data(&state_obj.data, set_data);
        if state == fsm.pseudo_root {
            let ds = self.global().lock().unwrap().environment.clone();
            self.set_from_state_data(&ds, true);
        }
    }

    /// Sets data from state data-store.\
    /// All data-elements contain script-source and needs to be evaluated by the datamodel before use.
    /// set_data - if true set the data, otherwise just initialize the variables.
    fn set_from_state_data(&mut self, data: &HashMap<String, DataArc>, set_data: bool);

    /// Initialize a global read-only variable.
    fn initialize_read_only(&mut self, name: &str, value: Data) {
        self.initialize_read_only_arc(name, create_data_arc(value));
    }

    fn initialize_read_only_arc(&mut self, name: &str, value: DataArc);

    /// Sets a global variable.
    fn set(&mut self, name: &str, data: Data, allow_undefined: bool) {
        self.set_arc(name, create_data_arc(data), allow_undefined);
    }

    fn set_arc(&mut self, name: &str, data: DataArc, allow_undefined: bool);

    // Sets system variable "_event"
    fn set_event(&mut self, event: &Event);

    /// Execute an assign expression.
    /// Returns true if the assignment was correct.
    fn assign(&mut self, left_expr: &Data, right_expr: &Data) -> bool;

    /// Gets a global variable by a location expression.\
    /// If the location is undefined or the location expression is invalid,
    /// "error.execute" shall be put inside the internal event queue.\
    /// See [internal_error_execution](Datamodel::internal_error_execution).
    fn get_by_location(&mut self, location: &str) -> Result<DataArc, String>;

    /// Convenient function to retrieve a value that has an alternative expression-value.\
    /// If value_expression is empty, Ok(value) is returned (if empty or not). If the expression
    /// results in error Err(message) and "error.execute" is put in internal queue.
    /// See [internal_error_execution](Datamodel::internal_error_execution).
    fn get_expression_alternative_value(&mut self, value: &Data, value_expression: &Data) -> Result<DataArc, String> {
        if value_expression.is_empty() {
            Ok(create_data_arc(value.clone()))
        } else {
            match self.execute(value_expression) {
                Err(_msg) => {
                    // Error -> Abort
                    Err("execution failed".to_string())
                }
                Ok(result) => Ok(result),
            }
        }
    }

    /// Get an _ioprocessor by name.
    fn get_io_processor(&mut self, name: &str) -> Option<Arc<Mutex<Box<dyn EventIOProcessor>>>> {
        self.global()
            .lock()
            .unwrap()
            .io_processors
            .get(name)
            .cloned()
    }

    /// Send an event via io-processor.
    /// Mainly here because of optimization reasons (spared copies).
    fn send(&mut self, ioc_processor: &str, target: &Data, event: Event) -> bool {
        let ioc = self.get_io_processor(ioc_processor);
        if let Some(ic) = ioc {
            let mut icg = ic.lock().unwrap();
            icg.send(self.global(), target.to_string().as_str(), event)
        } else {
            false
        }
    }

    /// Clear all data.
    fn clear(&mut self);

    /// "log" function, use for \<log\> content.
    fn log(&mut self, msg: &str) {
        info!("{}", msg);
    }

    /// Executes a script.\
    /// If the script execution fails, "error.execute" shall be put
    /// inside the internal event queue.
    /// See [internal_error_execution](Datamodel::internal_error_execution).
    fn execute(&mut self, script: &Data) -> Result<DataArc, String>;

    /// Executes a for-each loop
    fn execute_for_each(
        &mut self,
        array_expression: &Data,
        item: &str,
        index: &str,
        execute_body: &mut dyn FnMut(&mut dyn Datamodel) -> bool,
    ) -> bool;

    /// *W3C says*:\
    /// The set of operators in conditional expressions varies depending on the data model,
    /// but all data models must support the 'In()' predicate, which takes a state ID as its
    /// argument and returns true if the state machine is in that state.\
    /// Conditional expressions in conformant SCXML documents should not have side effects.
    /// #Actual Implementation:
    /// As no side effects shall occur, this method should be "&self". But we assume that most script-engines have
    /// no read-only "eval" function and such method may be hard to implement.
    fn execute_condition(&mut self, script: &Data) -> Result<bool, String>;

    /// Executes content by id.
    #[allow(non_snake_case)]
    fn executeContent(&mut self, fsm: &Fsm, contentId: ExecutableContentId) -> bool;

    /// *W3C says*:\
    /// Indicates that an error internal to the execution of the document has occurred, such as one
    /// arising from expression evaluation.
    fn internal_error_execution_with_event(&mut self, event: &Event) {
        get_global!(self).enqueue_internal(Event::error_execution_with_event(event));
    }

    /// *W3C says*:\
    /// Indicates that an error internal to the execution of the document has occurred, such as one
    /// arising from expression evaluation.
    fn internal_error_execution_for_event(&mut self, send_id: &Option<String>, invoke_id: &Option<InvokeId>) {
        get_global!(self).enqueue_internal(Event::error_execution(send_id, invoke_id));
    }

    /// *W3C says*:\
    /// Indicates that an error internal to the execution of the document has occurred, such as one
    /// arising from expression evaluation.
    fn internal_error_execution(&mut self) {
        get_global!(self).enqueue_internal(Event::error_execution(&None, &None));
    }

    /// *W3C says*:\
    /// W3C: Indicates that an error has occurred while trying to communicate with an external entity.
    fn internal_error_communication(&mut self, event: &Event) {
        get_global!(self).enqueue_internal(Event::error_communication(event));
    }

    /// Evaluates a content element.\
    /// Returns the static content or executes the expression.
    fn evaluate_content(&mut self, content: &Option<CommonContent>) -> Option<DataArc> {
        match content {
            None => None,
            Some(ct) => {
                match &ct.content_expr {
                    None => ct
                        .content
                        .as_ref()
                        .map(|ct_content| match ct_content.parse::<f64>() {
                            Ok(value) => match numeric_to_integer(&Data::Double(value)) {
                                Some(i) => create_data_arc(Data::Integer(i)),
                                None => create_data_arc(Data::Double(value)),
                            },
                            Err(_) => create_data_arc(Data::String(ct_content.clone())),
                        }),
                    Some(expr) => {
                        match self.execute(&str_to_source(expr.as_str())) {
                            Err(msg) => {
                                // W3C:\
                                // If the evaluation of 'expr' produces an error, the Processor must place
                                // error.execution in the internal event queue and use the empty string as
                                // the value of the <content> element.
                                error!("content expr '{}' is invalid ({})", expr, msg);
                                self.internal_error_execution();
                                None
                            }
                            Ok(value) => Some(value),
                        }
                    }
                }
            }
        }
    }

    /// Evaluates a list of Param-elements and
    /// returns the resulting data
    fn evaluate_params(&mut self, params: &Option<Vec<Parameter>>, values: &mut Vec<ParamPair>) {
        match &params {
            None => {}
            Some(params) => {
                for param in params {
                    if !param.location.is_empty() {
                        match self.get_by_location(&param.location) {
                            Err(msg) => {
                                // W3C:\
                                // If the 'location' attribute does not refer to a valid location in
                                // the data model, ..., the SCXML Processor must place the error
                                // 'error.execution' on the internal event queue and must ignore the name
                                // and value.
                                error!("location of param {} is invalid ({})", param, msg);
                                // get_by_location already added "error.execution"
                            }
                            Ok(value) => {
                                values.push(ParamPair::new_moved(
                                    param.name.clone(),
                                    value.lock().unwrap().clone(),
                                ));
                            }
                        }
                    } else if !param.expr.is_empty() {
                        match self.execute(&str_to_source(param.expr.as_str())) {
                            Err(msg) => {
                                //  W3C:\
                                // ...if the evaluation of the 'expr' produces an error, the SCXML
                                // Processor must place the error 'error.execution' on the internal event
                                // queue and must ignore the name and value.
                                error!("expr of param {} is invalid ({})", param, msg);
                                self.internal_error_execution();
                            }
                            Ok(value) => {
                                values.push(ParamPair::new_moved(
                                    param.name.clone(),
                                    value.lock().unwrap().clone(),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The "Null" Datamodel as specified by W3C. A minimal model without any data.
/// ## W3C says:
/// ### B.1 The Null Data Model
/// The value "null" for the 'datamodel' attribute results in an absent or empty data model. In particular:
/// - B.1.1 Data Model
///
///   There is no underlying data model.
/// - B.1.2 Conditional Expressions
///
///   The boolean expression language consists of the In predicate only. It has the form 'In(id)',
///   where id is the id of a state in the enclosing state machine.
///   The predicate must return 'true' if and only if that state is in the current state configuration.
/// - B.1.3 Location Expressions
///
///   There is no location expression language.
/// - B.1.4 Value Expressions
///
///   There is no value expression language.
/// - B.1.5 Scripting
///
///   There is no scripting language.
/// - B.1.6 System Variables
///
///   System variables are not accessible.
/// - B.1.7 Unsupported Elements
///
///   The \<foreach\> element and the elements defined in 5 Data Model and Data Manipulation are not
///   supported in the Null Data Model.
pub struct NullDatamodel {
    pub global: GlobalDataArc,
    pub state_name_to_id: HashMap<String, StateId>,
    pub actions: ActionMap,
}

/// The Factory to create a new Null-Datamodel in case an FSM uses it.
pub struct NullDatamodelFactory {}

impl DatamodelFactory for NullDatamodelFactory {
    fn create(&mut self, global_data: GlobalDataArc, _options: &HashMap<String, String>) -> Box<dyn Datamodel> {
        Box::new(NullDatamodel::new(global_data))
    }
}

impl NullDatamodel {
    pub fn new(global_data: GlobalDataArc) -> NullDatamodel {
        NullDatamodel {
            global: global_data,
            state_name_to_id: HashMap::new(),
            actions: HashMap::new(),
        }
    }
}

impl Datamodel for NullDatamodel {
    fn global(&mut self) -> &mut GlobalDataArc {
        &mut self.global
    }

    fn global_s(&self) -> &GlobalDataArc {
        &self.global
    }

    fn get_name(&self) -> &str {
        NULL_DATAMODEL
    }

    fn add_functions(&mut self, fsm: &mut Fsm) {
        // TODO: Add actions
        for state in fsm.states.as_slice() {
            self.state_name_to_id.insert(state.name.clone(), state.id);
        }
        // self.actions =  actions.get_map_copy()
    }

    fn set_ioprocessors(&mut self) {
        // nothing to do
    }

    #[allow(non_snake_case)]
    fn initializeDataModel(&mut self, _fsm: &mut Fsm, _dataState: StateId, _set_data: bool) {
        // nothing to do
    }

    fn set_from_state_data(&mut self, _data: &HashMap<String, DataArc>, _set_data: bool) {
        // nothing to do
    }

    fn initialize_read_only_arc(&mut self, _name: &str, _value: DataArc) {
        // nothing to do
    }

    fn set_arc(&mut self, _name: &str, _data: DataArc, _allow_undefined: bool) {
        // nothing to do
    }

    fn set_event(&mut self, _event: &Event) {
        // nothing to do
    }

    fn assign(&mut self, _left_expr: &Data, _right_expr: &Data) -> bool {
        // nothing to do
        true
    }

    fn get_by_location(&mut self, _name: &str) -> Result<DataArc, String> {
        Err("unimplemented".to_string())
    }

    fn clear(self: &mut NullDatamodel) {}

    fn log(self: &mut NullDatamodel, msg: &str) {
        println!("{}", msg);
    }

    fn execute(&mut self, _script: &Data) -> Result<DataArc, String> {
        Err("unimplemented".to_string())
    }

    fn execute_for_each(
        &mut self,
        _array_expression: &Data,
        _item: &str,
        _index: &str,
        _execute_body: &mut dyn FnMut(&mut dyn Datamodel) -> bool,
    ) -> bool {
        // nothing to do
        true
    }

    /// *W3C says*:
    /// The boolean expression language consists of the In predicate only.
    /// It has the form 'In(id)', where id is the id of a state in the enclosing state machine.
    /// The predicate must return 'true' if and only if that state is in the current state configuration.
    fn execute_condition(&mut self, script: &Data) -> Result<bool, String> {
        let mut lexer = ExpressionLexer::new(script.to_string());
        if lexer.next_token() == Token::Identifier("In".to_string()) && lexer.next_token() == Token::Bracket('(') {
            match lexer.next_token() {
                Token::TString(state_name) | Token::Identifier(state_name) => {
                    if lexer.next_token() != Token::Bracket(')') {
                        return Err("Matching ')' is missing".to_string());
                    } else {
                        return match self.state_name_to_id.get(&state_name) {
                            None => Err(format!("Illegal state name '{}'", state_name)),
                            Some(state_id) => Ok(self
                                .global
                                .lock()
                                .unwrap()
                                .configuration
                                .data
                                .contains(state_id)),
                        };
                    }
                }
                _ => {}
            }
        }
        Err("Syntax error".to_string())
    }

    #[allow(non_snake_case)]
    fn executeContent(&mut self, _fsm: &Fsm, _content_id: ExecutableContentId) -> bool {
        // Nothing
        true
    }
}

/// Converts the Data item to a string.
pub fn data_arc_to_string(data: &DataArc) -> Result<String, String> {
    match data.arc.try_lock() {
        Ok(val) => data_to_string(val.deref()),
        Err(_) => Err("locked".to_string()),
    }
}

/// Converts the Data item to a string.
pub fn data_to_string(data: &Data) -> Result<String, String> {
    match data {
        Data::Error(err) => Err(err.clone()),
        Data::None() => Ok("none".to_string()),
        Data::Null() => Ok("null".to_string()),
        Data::String(s) => Ok(s.clone()),
        Data::Source(s) => Ok(s.source.clone()),
        Data::Integer(i) => Ok(i.to_string()),
        Data::Double(f) => Ok(f.to_string()),
        Data::Boolean(b) => {
            let bs = if *b { "true" } else { "false" };
            Ok(bs.to_string())
        }
        Data::Array(a) => {
            let mut v = String::with_capacity(50);
            for da in a {
                match data_arc_to_string(da) {
                    Ok(da_string) => {
                        if !v.is_empty() {
                            v.push(',');
                        }
                        v.push_str(&da_string);
                    }
                    Err(err) => return Err(err),
                }
            }
            Ok(v)
        }
        Data::Map(m) => {
            let mut v = String::with_capacity(50);
            for (key, data_arc) in m {
                if !v.is_empty() {
                    v.push(',');
                }
                v.push_str(key);
                v.push(':');
                match data_arc_to_string(data_arc) {
                    Ok(s) => {
                        v.push_str(s.as_str());
                    }
                    Err(err) => return Err(err),
                }
            }
            Ok(v)
        }
    }
}

/// Implements a "+" operation on Data items.
pub fn operation_plus(left: &Data, right: &Data) -> Data {
    if left.is_numeric() && right.is_numeric() {
        match (left, right) {
            (Data::Double(d1), Data::Double(d2)) => Data::Double(d1 + d2),
            (Data::Integer(d1), Data::Double(d2)) => Data::Double((*d1 as f64) + d2),
            (Data::Double(d1), Data::Integer(d2)) => Data::Double(d1 + (*d2 as f64)),
            (Data::Integer(i1), Data::Integer(i2)) => Data::Integer(i1.saturating_add(*i2)),
            _ => Data::Error("Internal Error in '+' operation".to_string()),
        }
    } else {
        match (left, right) {
            (_, Data::Error(err)) | (Data::Error(err), _) => Data::Error(err.clone()),
            (Data::String(s), _) => {
                let mut r = s.clone();
                r.push_str(right.to_string().as_str());
                Data::String(r)
            }
            (Data::Source(s), _) => {
                let mut r = s.source.clone();
                r.push_str(right.to_string().as_str());
                Data::Source(SourceCode::new_move(r, 0))
            }
            (Data::Array(a1), Data::Array(a2)) => {
                let mut a1_copy = a1.clone();
                a1_copy.append(&mut a2.clone());
                Data::Array(a1_copy)
            }
            (Data::Array(a1), _) => {
                let mut a1_copy = a1.clone();
                a1_copy.push(create_data_arc(right.clone()));
                Data::Array(a1_copy)
            }
            (_, Data::String(s)) => {
                let mut r = left.to_string();
                r.push_str(s);
                Data::String(r)
            }
            (_, Data::Source(s)) => {
                let mut r = left.to_string();
                r.push_str(s.as_str());
                Data::Source(SourceCode::new_move(r, 0))
            }
            (Data::Map(m1), Data::Map(m2)) => {
                let mut m1_copy = m1.clone();
                m1_copy.extend(m2.clone());
                Data::Map(m1_copy)
            }
            (Data::Boolean(b1), Data::Boolean(b2)) => Data::Boolean(*b1 && *b2),
            _ => Data::Error("Wrong argument types for '+'".to_string()),
        }
    }
}

/// Implements a "&" operation on Data items.
pub fn operation_and(left: &Data, right: &Data) -> Data {
    match (left, right) {
        (_, Data::Error(err)) | (Data::Error(err), _) => Data::Error(err.clone()),
        (Data::Boolean(b1), Data::Boolean(b2)) => Data::Boolean(*b1 && *b2),
        _ => Data::Error("Wrong argument types for '&'".to_string()),
    }
}

/// Implements a "|" operation on Data items.
pub fn operation_or(left: &Data, right: &Data) -> Data {
    match (left, right) {
        (_, Data::Error(err)) | (Data::Error(err), _) => Data::Error(err.clone()),
        (Data::Boolean(b1), Data::Boolean(b2)) => Data::Boolean(*b1 || *b2),
        _ => Data::Error("Wrong argument types for '|'".to_string()),
    }
}

/// Implements a "-" operation on Data items.
pub fn operation_minus(left: &Data, right: &Data) -> Data {
    if left.is_numeric() && right.is_numeric() {
        match (left, right) {
            (Data::Double(d1), Data::Double(d2)) => Data::Double(d1 - d2),
            (Data::Integer(d1), Data::Double(d2)) => Data::Double((*d1 as f64) - d2),
            (Data::Double(d1), Data::Integer(d2)) => Data::Double(d1 - (*d2 as f64)),
            (Data::Integer(i1), Data::Integer(i2)) => Data::Integer(i1.saturating_sub(*i2)),
            _ => Data::Error("Internal Error in '-' operation".to_string()),
        }
    } else {
        Data::Error("Wrong argument types for '-'".to_string())
    }
}

/// Implements a "*" operation on Data items.
pub fn operation_multiply(left: &Data, right: &Data) -> Data {
    if left.is_numeric() && right.is_numeric() {
        match (left, right) {
            (Data::Double(d1), Data::Double(d2)) => Data::Double(d1 * d2),
            (Data::Integer(d1), Data::Double(d2)) => Data::Double((*d1 as f64) * d2),
            (Data::Double(d1), Data::Integer(d2)) => Data::Double((*d1) * (*d2 as f64)),
            (Data::Integer(i1), Data::Integer(i2)) => Data::Integer(i1.saturating_mul(*i2)),
            _ => Data::Error("Internal Error in '*' operation".to_string()),
        }
    } else {
        Data::Error("Wrong argument types for '*'".to_string())
    }
}

/// Implements a ":" operation on Data items.
pub fn operation_divide(left: &Data, right: &Data) -> Data {
    if left.is_numeric() && right.is_numeric() {
        let right_value = right.as_number();
        let r = left.as_number() / right_value;
        if r.is_nan() {
            // This covers also division by 0.
            Data::Error("Result of '/' is NaN".to_string())
        } else {
            Data::Double(r)
        }
    } else {
        Data::Error("Wrong argument types for '/'".to_string())
    }
}

/// Implements a "%" modulus (remainder) operation on Data items.
pub fn operation_modulus(left: &Data, right: &Data) -> Data {
    if left.is_numeric() && right.is_numeric() {
        match (left, right) {
            (Data::Double(d1), Data::Double(d2)) => Data::Double(d1 % d2),
            (Data::Integer(d1), Data::Double(d2)) => Data::Double((*d1 as f64) % d2),
            (Data::Double(d1), Data::Integer(d2)) => Data::Double((*d1) % (*d2 as f64)),
            (Data::Integer(i1), Data::Integer(i2)) => Data::Integer(i1 % i2),
            _ => Data::Error("Internal Error in '%' operation".to_string()),
        }
    } else {
        Data::Error("Wrong argument types for '%'".to_string())
    }
}

/// Implements a "<" (less) operation on Data items.
pub fn operation_less(left: &crate::datamodel::Data, right: &crate::datamodel::Data) -> crate::datamodel::Data {
    if left.is_numeric() && right.is_numeric() {
        Data::Boolean(left.as_number() < right.as_number())
    } else {
        match (left, right) {
            (Data::String(_) | Data::Source(_), Data::String(_) | Data::Source(_)) => {
                Data::Boolean(left.to_string() < right.to_string())
            }
            _ => {
                #[cfg(feature = "Debug")]
                warn!("'<' supports only numeric or string types");
                Data::Boolean(false)
            }
        }
    }
}

/// Implements a "<=" (less or equal) operation on Data items.
pub fn operation_less_equal(left: &crate::datamodel::Data, right: &crate::datamodel::Data) -> crate::datamodel::Data {
    if left.is_numeric() && right.is_numeric() {
        Data::Boolean(left.as_number() <= right.as_number())
    } else {
        match (left, right) {
            (Data::String(_) | Data::Source(_), Data::String(_) | Data::Source(_)) => {
                Data::Boolean(left.to_string() <= right.to_string())
            }
            _ => {
                #[cfg(feature = "Debug")]
                warn!("'<=' supports only numeric or string types");
                Data::Boolean(false)
            }
        }
    }
}

/// Implements a ">" (greater) operation on Data items.
pub fn operation_greater(left: &crate::datamodel::Data, right: &crate::datamodel::Data) -> crate::datamodel::Data {
    if left.is_numeric() && right.is_numeric() {
        Data::Boolean(left.as_number() > right.as_number())
    } else {
        match (left, right) {
            (Data::String(_) | Data::Source(_), Data::String(_) | Data::Source(_)) => {
                Data::Boolean(left.to_string() > right.to_string())
            }
            _ => Data::Error("'>' supports only numeric or string types".to_string()),
        }
    }
}

/// Implements a ">=" (greater or equal) operation on Data items.
pub fn operation_greater_equal(left: &Data, right: &Data) -> Data {
    if left.is_numeric() && right.is_numeric() {
        Data::Boolean(left.as_number() >= right.as_number())
    } else {
        match (left, right) {
            (Data::String(_) | Data::Source(_), Data::String(_) | Data::Source(_)) => {
                Data::Boolean(left.to_string() >= right.to_string())
            }
            _ => {
                #[cfg(feature = "Debug")]
                warn!("'>=' supports only numeric or string types");
                Data::Boolean(false)
            }
        }
    }
}

/// Implements a "==" (equal) operation on Data items.
pub fn operation_equal(left: &Data, right: &Data) -> Data {
    Data::Boolean(left.eq(right))
}

/// Implements a "!=" (not equal) operation on Data items.
pub fn operation_not_equal(left: &Data, right: &Data) -> Data {
    Data::Boolean(!left.eq(right))
}

pub trait ToAny: 'static {
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn as_any(&self) -> &dyn Any;
}

impl<T: Debug + 'static> ToAny for T {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub type SourceId = usize;

/// A Wrapper for source script code with a unique Id for effective identification.
#[derive(Clone)]
pub struct SourceCode {
    pub source: String,

    /// The unique Id of the script. Unique only inside the current life-cycle.\
    /// Invalid if 0-
    pub source_id: SourceId,
}

impl SourceCode {
    pub fn new(source: &str, source_id: SourceId) -> SourceCode {
        SourceCode {
            source: source.to_string(),
            source_id,
        }
    }

    pub fn new_move(source: String, source_id: SourceId) -> SourceCode {
        SourceCode { source, source_id }
    }

    pub fn is_empty(&self) -> bool {
        self.source.is_empty()
    }

    pub fn as_str(&self) -> &str {
        self.source.as_str()
    }

    pub fn len(&self) -> usize {
        self.source.len()
    }
}

impl Display for SourceCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.source)
    }
}

/// Data Variant used to handle data in a type-safe but Datamodel-agnostic way.
#[derive(Clone)]
pub enum Data {
    Integer(i64),
    Double(f64),
    String(String),
    Boolean(bool),
    Array(Vec<DataArc>),
    /// A map, can also be used to store "object"-like data-structures.
    Map(HashMap<String, DataArc>),
    Null(),
    /// Special placeholder to indicate an error
    Error(String),
    /// Special placeholder to indicate script source (from FSM definition) that needs to be evaluated by the datamodel.
    Source(SourceCode),
    /// Special placeholder to indicate empty content.
    None(),
}

/// Create a Data::Source from a str with invalid id.\
/// Should be used for calculated script source, that is not part of FSM definition.
pub fn str_to_source(str: &str) -> Data {
    Data::Source(SourceCode::new(str, 0))
}

/// Tries to convert the numeric data to an integer value.
pub fn numeric_to_integer(data: &Data) -> Option<i64> {
    match data {
        Data::Integer(value) => Some(*value),
        Data::Double(value_ref) => {
            let value = *value_ref;
            if value.fract().abs() < 0.001 && value >= i64::MIN as f64 && value <= i64::MAX as f64 {
                Some(value as i64)
            } else {
                None
            }
        }
        Data::String(_)
        | Data::Boolean(_)
        | Data::Array(_)
        | Data::Map(_)
        | Data::Null()
        | Data::Error(_)
        | Data::Source(_)
        | Data::None() => None,
    }
}

impl PartialEq for Data {
    fn eq(&self, other: &Self) -> bool {
        if std::ptr::eq(&self, &other) {
            true
        } else {
            match (self, other) {
                (Data::Integer(a), Data::Double(b)) => (*a as f64) == *b,
                (Data::Integer(a), Data::Integer(b)) => *a == *b,
                (Data::Double(a), Data::Double(b)) => *a == *b,
                (Data::Double(a), Data::Integer(b)) => *a == (*b as f64),
                (Data::String(a), Data::String(b)) => *a == *b,
                (Data::Boolean(a), Data::Boolean(b)) => *a == *b,
                (Data::Array(a), Data::Array(b)) => {
                    if a.len() != b.len() {
                        return false;
                    }
                    for index in 0..a.len() {
                        // Use deadlock-free eq function of DataArc.
                        if !a[index].eq(&b[index]) {
                            return false;
                        }
                    }
                    true
                }
                (Data::Map(a), Data::Map(b)) => {
                    if a.len() != b.len() {
                        return false;
                    }
                    for (key, value) in a {
                        if let Some(other_value) = b.get(key) {
                            // Use deadlock-free eq function of DataArc.
                            if !value.eq(other_value) {
                                return false;
                            }
                        } else {
                            return false;
                        }
                    }
                    true
                }
                (Data::Null(), Data::Null()) => true,
                (Data::Error(a), Data::Error(b)) => a == b,
                (Data::Source(a), Data::Source(b)) => a.source == b.source,
                (Data::None(), Data::None()) => true,
                _ => false,
            }
        }
    }
}

impl Display for Data {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Data::Integer(v) => {
                write!(f, "{}", v)
            }
            Data::Double(v) => {
                write!(f, "{}", v)
            }
            Data::String(v) => {
                // TODO: Escape
                write!(f, "{}", v)
            }
            Data::Boolean(v) => {
                write!(f, "{}", v)
            }
            Data::Array(a) => {
                write!(f, "{}", vec_to_string(a))
            }
            Data::Map(m) => {
                let mut b = String::with_capacity(100);
                b.push('{');
                let mut first = true;
                for (key, data) in m {
                    if first {
                        first = false;
                    } else {
                        b.push(',');
                    }
                    b.push('\'');
                    // TODO: Escape
                    b.push_str(key);
                    b.push_str("':");
                    b.push_str(format!("{}", data).as_str())
                }
                b.push('}');
                write!(f, "{}", b)
            }
            Data::Null() => {
                write!(f, "null")
            }
            Data::Error(err) => {
                write!(f, "Error {}", err)
            }
            Data::Source(src) => {
                write!(f, "{}", src)
            }
            Data::None() => {
                write!(f, "")
            }
        }
    }
}

impl Data {
    pub fn as_number(&self) -> f64 {
        match self {
            Data::Integer(v) => *v as f64,
            Data::Double(v) => *v,
            Data::String(s) => s.parse::<f64>().unwrap_or(0f64),
            Data::Boolean(b) => {
                if *b {
                    1f64
                } else {
                    0f64
                }
            }
            Data::Array(a) => a.len() as f64,
            Data::Map(a) => a.len() as f64,
            Data::Null() => 0f64,
            Data::Error(_) => 0f64,
            Data::Source(src) => {
                let r = src.source.parse::<f64>();
                r.unwrap_or(0f64)
            }
            Data::None() => 0f64,
        }
    }

    pub fn as_script(&self) -> String {
        match self {
            Data::Integer(v) => v.to_string(),
            Data::Double(v) => v.to_string(),
            Data::String(s) => {
                format!("'{}'", s)
            }
            Data::Boolean(b) => (if *b { "true" } else { "false" }).to_string(),
            Data::Array(_) => self.to_string(),
            Data::Map(_) => self.to_string(),
            Data::Null() => "null".to_string(),
            Data::Error(_) => "".to_string(),
            Data::Source(s) => s.source.clone(),
            Data::None() => "".to_string(),
        }
    }

    pub fn is_numeric(&self) -> bool {
        match self {
            Data::Integer(_) => true,
            Data::Double(_) => true,
            Data::String(_) => false,
            Data::Boolean(_) => false,
            Data::Array(_) => false,
            Data::Map(_) => false,
            Data::Null() => true,
            Data::Error(_) => false,
            Data::Source(_) => false,
            Data::None() => false,
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Data::Boolean(_) | Data::Integer(_) | Data::Double(_) => false,
            Data::String(s) => s.is_empty(),
            Data::Array(a) => a.is_empty(),
            Data::Map(m) => m.is_empty(),
            Data::Null() => true,
            Data::Error(_) => true,
            Data::Source(s) => s.is_empty(),
            Data::None() => true,
        }
    }
}

impl Debug for Data {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self) // Display
    }
}

impl Default for Data {
    fn default() -> Self {
        Data::Null()
    }
}

pub const DATA_FLAG_READONLY: u8 = 1u8;

/// Wrapper that maintains an Arx to a Data element.
#[derive(Clone)]
pub struct DataArc {
    pub arc: Arc<Mutex<Data>>,
    pub flags: u8,
}

impl DataArc {
    fn print(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self.arc.try_lock() {
            Ok(val) => {
                write!(f, "{}", val.deref())
            }
            Err(_) => {
                write!(f, "<locked arc>")
            }
        }
    }

    pub fn lock(&self) -> LockResult<MutexGuard<'_, Data>> {
        self.arc.lock()
    }

    pub fn is_readonly(&self) -> bool {
        (self.flags & DATA_FLAG_READONLY) != 0
    }

    pub fn set_readonly(&mut self, read_only: bool) {
        if read_only {
            self.flags |= DATA_FLAG_READONLY;
        } else {
            self.flags &= !DATA_FLAG_READONLY;
        }
    }
}

impl PartialEq for DataArc {
    fn eq(&self, other: &Self) -> bool {
        // It's really important to check first of both arc reference the same object, otherwise the compare
        // a deadlock will occur.
        Arc::ptr_eq(&self.arc, &other.arc) || self.arc.lock().unwrap().eq(other.lock().unwrap().deref())
    }
}

impl Display for DataArc {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.print(f)
    }
}

impl std::fmt::Debug for DataArc {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.print(f)
    }
}

pub fn create_data_arc(data: Data) -> DataArc {
    DataArc {
        arc: Arc::new(Mutex::from(data)),
        flags: 0,
    }
}

/// Wrapper to maintain the map of current data elements for one FSM.
#[derive(Debug)]
pub struct DataStore {
    pub map: HashMap<String, DataArc>,
}

impl Default for DataStore {
    fn default() -> Self {
        DataStore::new()
    }
}

impl DataStore {
    pub fn new() -> DataStore {
        DataStore {
            map: HashMap::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<DataArc> {
        #[allow(clippy::manual_map)]
        match self.map.get(key) {
            None => None,
            Some(v) => Some(v.clone()),
        }
    }

    pub fn set(&mut self, key: String, data: Data) -> bool {
        self.set_arc(key, create_data_arc(data))
    }

    pub fn set_arc(&mut self, key: String, data: DataArc) -> bool {
        // W3C want to assign only to defined variables.
        if let std::collections::hash_map::Entry::Occupied(mut old) = self.map.entry(key) {
            if old.get().is_readonly() {
                #[cfg(feature = "Debug")]
                debug!("Can't set read-only {}", old.key());
                false
            } else {
                old.insert(data);
                true
            }
        } else {
            false
        }
    }

    pub fn set_undefined(&mut self, key: String, data: Data) {
        self.set_undefined_arc(key, create_data_arc(data));
    }

    pub fn set_undefined_arc(&mut self, key: String, data: DataArc) {
        match self.map.entry(key) {
            Entry::Occupied(mut old) => {
                if old.get().is_readonly() {
                    #[cfg(feature = "Debug")]
                    debug!("Can't set read-only {}", old.key());
                } else {
                    old.insert(data);
                }
            }
            Entry::Vacant(x) => {
                x.insert(data);
            }
        }
    }
}
