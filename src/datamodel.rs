//! Defines the API used to access the data models.

use std::any::Any;
use std::collections::HashMap;
use std::fmt::{Debug, Display, Formatter};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::actions::ActionMap;
use log::error;

use crate::event_io_processor::EventIOProcessor;
use crate::expression_parser::{ExpressionLexer, Token};
use crate::fsm::{
    vec_to_string, CommonContent, Event, ExecutableContentId, Fsm, GlobalData, InvokeId, ParamPair, Parameter, StateId,
};

pub const DATAMODEL_OPTION_PREFIX: &str = "datamodel:";

pub const NULL_DATAMODEL: &str = "NULL";
pub const NULL_DATAMODEL_LC: &str = "null";

pub const SCXML_INVOKE_TYPE: &str = "http://www.w3.org/TR/scxml";

/// W3C: Processors MAY define short form notations as an authoring convenience
/// (e.g., "scxml" as equivalent to http://www.w3.org/TR/scxml/).
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

/// Gets the global data store from datamodel.
#[macro_export]
macro_rules! get_global {
    ($x:expr) => {
        $x.global().lock()
    };
}

pub type GlobalDataLock<'a> = MutexGuard<'a, GlobalData>;

/// Currently we assume that we need access to the global-data via a mutex.
/// If not, change this type to "GlobalData" and adapt implementation.
#[derive(Clone)]
pub struct GlobalDataArc {
    arc: Arc<Mutex<GlobalData>>,
}

impl Default for GlobalDataArc {
    fn default() -> Self {
        GlobalDataArc::new()
    }
}

impl GlobalDataArc {
    pub fn new() -> GlobalDataArc {
        GlobalDataArc {
            arc: Arc::new(Mutex::new(GlobalData::new())),
        }
    }

    pub fn lock(&self) -> GlobalDataLock {
        self.arc.lock().unwrap()
    }
}

/// Data model interface trait.
/// *W3C says*:
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
    /// accessing data of parents (Fsm in this case) from inside a member (the actual Datamodel), most global data is
    /// store in the "GlobalData" struct that is owned by the data model.
    fn global(&mut self) -> &mut GlobalDataArc;

    fn global_s(&self) -> &GlobalDataArc;

    /// Get the name of the data model as defined by the \<scxml\> attribute "datamodel".
    fn get_name(&self) -> &str;

    /// Adds the "In" and other function.\
    /// If needed, adds also "log" function and sets '_ioprocessors'.
    fn add_functions(&mut self, fsm: &mut Fsm);

    /// Initialize the data model for one data-store.
    /// This method is called for the global data and for the data of each state.
    #[allow(non_snake_case)]
    fn initializeDataModel(&mut self, fsm: &mut Fsm, state: StateId, set_data: bool);

    /// Initialize a global read-only variable.
    fn initialize_read_only(&mut self, name: &str, value: &str);

    /// Sets a global variable.
    fn set(&mut self, name: &str, data: Data);

    // Sets system variable "_event"
    fn set_event(&mut self, event: &Event);

    /// Execute an assign expression.
    /// Returns true if the assignment was correct.
    fn assign(&mut self, left_expr: &str, right_expr: &str) -> bool;

    /// Gets a global variable by a location expression.\
    /// If the location is undefined or the location expression is invalid,
    /// "error.execute" shall be put inside the internal event queue.\
    /// See [internal_error_execution](Datamodel::internal_error_execution).
    fn get_by_location(&mut self, location: &str) -> Result<Data, String>;

    /// Convenient function to retrieve a value that has an alternative expression-value.\
    /// If value_expression is empty, Ok(value) is returned (if empty or not). If the expression
    /// results in error Err(message) and "error.execute" is put in internal queue.
    /// See [internal_error_execution](Datamodel::internal_error_execution).
    fn get_expression_alternative_value(&mut self, value: &str, value_expression: &str) -> Result<String, String> {
        if value_expression.is_empty() {
            Ok(value.to_string())
        } else {
            match self.execute(value_expression) {
                Err(_msg) => {
                    // Error -> Abort
                    Err("execution failed".to_string())
                }
                Ok(value) => Ok(value),
            }
        }
    }

    /// Get an _ioprocessor by name.
    fn get_io_processor(&mut self, name: &str) -> Option<Arc<Mutex<Box<dyn EventIOProcessor>>>>;

    /// Send an event via io-processor.
    /// Mainly here because of optimization reasons (spared copies).
    fn send(&mut self, ioc_processor: &str, target: &str, event: Event) -> bool;

    /// Get a modifiable data element by name.
    fn get_mut(&mut self, name: &str) -> Option<&mut Data>;

    /// Clear all data.
    fn clear(&mut self);

    /// "log" function, use for \<log\> content.
    fn log(&mut self, msg: &str);

    /// Executes a script.\
    /// If the script execution fails, "error.execute" shall be put
    /// inside the internal event queue.
    /// See [internal_error_execution](Datamodel::internal_error_execution).
    fn execute(&mut self, script: &str) -> Result<String, String>;

    /// Executes a for-each loop
    fn execute_for_each(
        &mut self,
        array_expression: &str,
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
    fn execute_condition(&mut self, script: &str) -> Result<bool, String>;

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
    fn evaluate_content(&mut self, content: &Option<CommonContent>) -> Option<String> {
        match content {
            None => None,
            Some(ct) => {
                match &ct.content_expr {
                    None => ct.content.clone(),
                    Some(expr) => {
                        match self.execute(expr.as_str()) {
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
                                values.push(ParamPair::new_moved(param.name.clone(), value));
                            }
                        }
                    } else if !param.expr.is_empty() {
                        match self.execute(param.expr.as_str()) {
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
                                    Data::String(value),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
}

/// ## W3C says:
/// ###B.1 The Null Data Model
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

    #[allow(non_snake_case)]
    fn initializeDataModel(&mut self, _fsm: &mut Fsm, _dataState: StateId, _set_data: bool) {
        // nothing to do
    }

    fn initialize_read_only(&mut self, _name: &str, _value: &str) {
        // nothing to do
    }

    fn set(&mut self, _name: &str, _data: Data) {
        // nothing to do
    }

    fn set_event(&mut self, _event: &Event) {
        // nothing to do
    }

    fn assign(&mut self, _left_expr: &str, _right_expr: &str) -> bool {
        // nothing to do
        true
    }

    fn get_by_location(&mut self, _name: &str) -> Result<Data, String> {
        Err("unimplemented".to_string())
    }

    fn get_io_processor(&mut self, name: &str) -> Option<Arc<Mutex<Box<dyn EventIOProcessor>>>> {
        self.global.lock().io_processors.get(name).cloned()
    }

    fn send(&mut self, ioc_processor: &str, target: &str, event: Event) -> bool {
        let ioc = self.get_io_processor(ioc_processor);
        if let Some(ic) = ioc {
            let mut icg = ic.lock().unwrap();
            icg.send(&self.global, target, event)
        } else {
            false
        }
    }

    fn get_mut(&mut self, _name: &str) -> Option<&mut Data> {
        None
    }

    fn clear(self: &mut NullDatamodel) {}

    fn log(self: &mut NullDatamodel, msg: &str) {
        println!("{}", msg);
    }

    fn execute(&mut self, _script: &str) -> Result<String, String> {
        Err("unimplemented".to_string())
    }

    fn execute_for_each(
        &mut self,
        _array_expression: &str,
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
    fn execute_condition(&mut self, script: &str) -> Result<bool, String> {

        let mut lexer = ExpressionLexer::new(script.to_string());
        if lexer.next_token() == Token::Identifier("In".to_string()) &&
            lexer.next_token() == Token::Bracket('(') {
            match lexer.next_token() {
                Token::TString(state_name) |
                Token::Identifier(state_name) => {
                    if lexer.next_token() != Token::Bracket(')') {
                        return Err("Matching ')' is missing".to_string());
                    } else {
                        return match self.state_name_to_id.get(&state_name) {
                            None => Err(format!("Illegal state name '{}'", state_name)),
                            Some(state_id) => Ok(self.global.lock().configuration.data.contains(state_id))
                        }
                    }
                }
                _ => {}
            }
        }
        return Err("Syntax error".to_string());
    }

    #[allow(non_snake_case)]
    fn executeContent(&mut self, _fsm: &Fsm, _content_id: ExecutableContentId) -> bool {
        // Nothing
        true
    }
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

/// Data Variant used to handle data type-safe but
/// Datamodel-agnostic way.
#[derive(Clone)]
pub enum Data {
    Integer(i64),
    Double(f64),
    String(String),
    Boolean(bool),
    Array(Vec<Data>),
    Map(HashMap<String, Data>),
    Null(),
}

impl Debug for Data {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self) // Display
    }
}

impl Display for Data {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self {
            Data::String(v) => {
                write!(f, "'{}'", v)
            }
            Data::Integer(v) => {
                write!(f, "{}", v)
            }
            Data::Double(v) => {
                write!(f, "{}", v)
            }
            Data::Boolean(v) => {
                write!(f, "{}", v)
            }
            Data::Array(v) => {
                write!(f, "{}", vec_to_string(v))
            }
            Data::Map(v) => {
                write!(f, "{:?}", v)
            }
            Data::Null() => {
                write!(f, "Null")
            }
        }
    }
}

#[derive(Debug)]
pub struct DataStore {
    pub values: HashMap<String, Data>,
}

impl Default for DataStore {
    fn default() -> Self {
        DataStore::new()
    }
}

impl DataStore {
    pub fn new() -> DataStore {
        DataStore {
            values: HashMap::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&Data> {
        if self.values.contains_key(key) {
            self.values.get(key)
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Data> {
        if self.values.contains_key(key) {
            self.values.get_mut(key)
        } else {
            None
        }
    }

    pub fn set(&mut self, key: &str, data: Data) {
        self.values.insert(key.to_string(), data);
    }
}
