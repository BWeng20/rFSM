//! Defines the API used to access the data models.

use std::any::Any;
use std::collections::HashMap;
use std::fmt::{Debug, Display, Formatter};
#[cfg(test)]
use std::println as info;
use std::sync::Arc;

#[cfg(not(test))]
use log::info;

use crate::event_io_processor::EventIOProcessor;
use crate::fsm;
use crate::fsm::{Event, ExecutableContentId, Fsm, GlobalData, StateId};

pub const NULL_DATAMODEL: &str = "NULL";
pub const NULL_DATAMODEL_LC: &str = "null";

pub const SCXML_TYPE: &str = "http://www.w3.org/TR/scxml/";

pub const SCXML_EVENT_PROCESSOR: &str = "http://www.w3.org/TR/scxml/#SCXMLEventProcessor";
pub const BASIC_HTTP_EVENT_PROCESSOR: &str = "http://www.w3.org/TR/scxml/#BasicHTTPEventProcessor";

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



/// Gets the global data store from a GlobalDataAccess.
#[macro_export]
macro_rules! access_global {
    ($x:expr) => {
       $x.lock().unwrap()
    }
}



/// Gets the global data store from datamodel.
#[macro_export]
macro_rules! get_global {
    ($x:expr) => {
       $x.global().lock().unwrap()
    }
}

/// Currently we assume that we need access to the global-data via a mutex.
/// If not, change this type to "GlobalData" and adapt macros access_global and get_global above.
pub type GlobalDataAccess = Arc<std::sync::Mutex<GlobalData>>;

/// Data model interface trait.
/// #W3C says:
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
    fn global(&mut self) -> &mut GlobalDataAccess;

    fn global_s(&self) -> &GlobalDataAccess;

    /// Get the name of the data model as defined by the \<scxml\> attribute "datamodel".
    fn get_name(self: &Self) -> &str;

    /// Adds the "In" function.
    fn implement_mandatory_functionality(&mut self, fsm: &mut Fsm);

    /// Initialize the data model for one data-store.
    /// This method is called for the global data and for the data of each state.
    #[allow(non_snake_case)]
    fn initializeDataModel(&mut self, fsm: &mut Fsm, state: StateId);

    /// Sets a global variable.
    fn set(&mut self, name: &str, data: Box<Data>);

    // Sets system variable "_event"
    fn set_event(&mut self, event: &fsm::Event);

    /// Execute an assign expression.
    fn assign(&mut self, left_expr: &str, right_expr: &str);

    /// Gets a global variable by a location expression.\
    /// If the location is undefined or the location expression is invalid,
    /// "error.execute" shall be put inside the internal event queue.\
    /// See [internal_error_execution](Datamodel::internal_error_execution).
    fn get_by_location(&mut self, location: &str) -> Option<Data>;

    /// Convenient function to retrieve a value that has an alternative expression-value.\
    /// If value_expression is empty, Ok(value) is returned (if empty or not). If the expression
    /// results in error Err(message) and "error.execute" is put in internal queue.
    /// See [internal_error_execution](Datamodel::internal_error_execution).
    fn get_expression_alternative_value(&mut self, value: &String, value_expression: &String) -> Result<String, String> {
        if value_expression.is_empty() {
            Ok(value.clone())
        } else {
            match self.execute(value_expression.as_str()) {
                None => {
                    // Error -> Abort
                    Err("execution failed".to_string())
                }
                Some(value) => {
                    Ok(value)
                }
            }
        }
    }

    /// Get _ioprocessors.
    fn get_io_processors(&mut self) -> &mut HashMap<String, Box<dyn EventIOProcessor>>;

    fn get_mut<'v>(&'v mut self, name: &str) -> Option<&'v mut Data>;

    /// Clear all.
    fn clear(&mut self);

    /// "log" function, use for \<log\> content.
    fn log(&mut self, msg: &str);

    /// Executes a script.\
    /// If the script execution fails, "error.execute" shall be put
    /// inside the internal event queue.
    /// See [internal_error_execution](Datamodel::internal_error_execution).
    fn execute(&mut self, script: &str) -> Option<String>;

    fn execute_for_each(&mut self, array_expression: &str, item: &str, index: &str,
                        execute_body: &mut dyn FnMut(&mut dyn Datamodel));

    /// #W3C says:
    /// The set of operators in conditional expressions varies depending on the data model,
    /// but all data models must support the 'In()' predicate, which takes a state ID as its
    /// argument and returns true if the state machine is in that state.\
    /// Conditional expressions in conformant SCXML documents should not have side effects.
    /// #Actual Implementation:
    /// As no side-effects shall occur, this method should be "&self". But we assume that most script-engines have
    /// no read-only "eval" function and such method may be hard to implement.
    fn execute_condition(&mut self, script: &str) -> Result<bool, String>;

    #[allow(non_snake_case)]
    fn executeContent(&mut self, fsm: &Fsm, contentId: ExecutableContentId);

    fn internal_error_execution(&mut self) {
        info!("enqueue error:execution");
        get_global!(self).internalQueue.enqueue(Event::error_execution());
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
#[derive(Debug)]
pub struct NullDatamodel {
    pub global: GlobalDataAccess,
    pub io_processors: HashMap<String, Box<dyn EventIOProcessor>>,
}

impl NullDatamodel {
    pub fn new() -> NullDatamodel {
        NullDatamodel {
            global: Arc::new(std::sync::Mutex::new(GlobalData::new())),
            io_processors: HashMap::new(),
        }
    }
}

impl Datamodel for NullDatamodel {
    fn global(&mut self) -> &mut GlobalDataAccess {
        &mut self.global
    }

    fn global_s(&self) -> &GlobalDataAccess {
        &self.global
    }

    fn get_name(self: &Self) -> &str {
        return NULL_DATAMODEL;
    }

    fn implement_mandatory_functionality(self: &mut Self, _fsm: &mut Fsm) {
        // TODO
    }

    #[allow(non_snake_case)]
    fn initializeDataModel(self: &mut Self, _fsm: &mut Fsm, _dataState: StateId) {
        // nothing to do
    }

    fn set(self: &mut NullDatamodel, _name: &str, _data: Box<Data>) {
        // nothing to do
    }

    fn set_event(&mut self, _event: &Event) {
        // nothing to do
    }

    fn assign(self: &mut NullDatamodel, _left_expr: &str, _right_expr: &str) {
        // nothing to do
    }

    fn get_by_location(self: &mut NullDatamodel, _name: &str) -> Option<Data> {
        None
    }

    fn get_io_processors(&mut self) -> &mut HashMap<String, Box<dyn EventIOProcessor>> {
        return &mut self.io_processors;
    }

    fn get_mut<'v>(&'v mut self, _name: &str) -> Option<&'v mut Data> {
        None
    }

    fn clear(self: &mut NullDatamodel) {}

    fn log(self: &mut NullDatamodel, msg: &str) {
        info!("Log: {}", msg);
    }

    fn execute(&mut self, _script: &str) -> Option<String> {
        None
    }

    fn execute_for_each(&mut self, _array_expression: &str, _item: &str, _index: &str,
                        _execute_body: &mut dyn FnMut(&mut dyn Datamodel)) {
        // nothing to do
    }

    /// #W3C says:
    /// The boolean expression language consists of the In predicate only.
    /// It has the form 'In(id)', where id is the id of a state in the enclosing state machine.
    /// The predicate must return 'true' if and only if that state is in the current state configuration.
    fn execute_condition(&mut self, _script: &str) -> Result<bool, String> {
        // TODO: Support for "In" predicate
        Ok(false)
    }

    #[allow(non_snake_case)]
    fn executeContent(&mut self, _fsm: &Fsm, _content_id: ExecutableContentId) {
        // Nothing
    }
}

pub trait ToAny: 'static {
    fn as_any(&mut self) -> &mut dyn Any;
}

impl<T: Debug + 'static> ToAny for T {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

pub struct Data {
    pub value: Option<String>,
}

impl Data {
    pub fn new(val: &str) -> Data {
        Data {
            value: Some(val.to_string()),
        }
    }
    pub fn new_moved(val: String) -> Data {
        Data {
            value: Some(val),
        }
    }
}

impl Debug for Data {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}",
               match &self.value {
                   Some(v) => {
                       v.as_str()
                   }
                   None => {
                       "null"
                   }
               })
    }
}

impl Display for Data {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match &self.value {
            Some(v) => {
                v.clone()
            }
            None => {
                "null".to_string()
            }
        };
        write!(f, "{}", str)
    }
}

#[derive(Debug)]
pub struct DataStore {
    pub values: HashMap<String, Box<Data>>,

}

impl DataStore {
    pub fn new() -> DataStore {
        DataStore {
            values: HashMap::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&Box<Data>> {
        if self.values.contains_key(key) {
            self.values.get(key)
        } else {
            None
        }
    }

    pub fn get_mut<'v>(&'v mut self, key: &str) -> Option<&'v mut Box<Data>> {
        if self.values.contains_key(key) {
            self.values.get_mut(key)
        } else {
            None
        }
    }

    pub fn set(&mut self, key: &str, data: Box<Data>) {
        self.values.insert(key.to_string(), data);
    }
}