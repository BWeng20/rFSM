//! Defines the API used to access the data models.

use std::any::Any;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};

use crate::event_io_processor::EventIOProcessor;
use crate::fsm::{ExecutableContentId, Fsm, GlobalData, StateId};

pub const NULL_DATAMODEL: &str = "NULL";
pub const NULL_DATAMODEL_LC: &str = "null";

pub const SCXML_EVENT_PROCESSOR: &str = "http://www.w3.org/TR/scxml/#SCXMLEventProcessor";
pub const BASIC_HTTP_EVENT_PROCESSOR: &str = "http://www.w3.org/TR/scxml/#BasicHTTPEventProcessor";


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
    fn global(&mut self) -> &mut GlobalData;

    fn global_s(&self) -> &GlobalData;

    /// Get the name of the data model as defined by the \<scxml\> attribute "datamodel".
    fn get_name(self: &Self) -> &str;

    /// Initialize the data model for one data-store.
    /// This method is called for the global data and for the data of each state.
    #[allow(non_snake_case)]
    fn initializeDataModel(&mut self, fsm: &mut Fsm, state: StateId);

    /// Sets a global variable.
    fn set(&mut self, name: &str, data: Box<dyn Data>);

    /// Gets a global variable.
    fn get(&self, name: &str) -> Option<&dyn Data>;

    /// Get _ioprocessors.
    fn get_io_processors(&mut self) -> &mut HashMap<String, Box<dyn EventIOProcessor>>;

    fn get_mut<'v>(&'v mut self, name: &str) -> Option<&'v mut dyn Data>;

    /// Clear all.
    fn clear(&mut self);

    /// "log" function, use for \<log\> content.
    fn log(&mut self, msg: &str);

    /// Execute a script.
    fn execute(&mut self, fsm: &Fsm, script: &str) -> String;

    fn execute_for_each(&mut self, fsm: &Fsm, array_expression: &str, item: &str, index: &str,
                        execute_body: &mut dyn FnMut(&mut dyn Datamodel));

    /// #W3C says:
    /// The set of operators in conditional expressions varies depending on the data model,
    /// but all data models must support the 'In()' predicate, which takes a state ID as its
    /// argument and returns true if the state machine is in that state.\
    /// Conditional expressions in conformant SCXML documents should not have side effects.
    /// #Actual Implementation:
    /// As no side-effects shall occur, this method should be "&self". But we assume that most script-engines have
    /// no read-only "eval" function and such method may be hard to implement.
    fn execute_condition(&mut self, fsm: &Fsm, script: &str) -> Result<bool, String>;

    #[allow(non_snake_case)]
    fn executeContent(&mut self, fsm: &Fsm, contentId: ExecutableContentId);
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
    pub global: GlobalData,
    pub io_processors: HashMap<String, Box<dyn EventIOProcessor>>,
}

impl NullDatamodel {
    pub fn new() -> NullDatamodel {
        NullDatamodel {
            global: GlobalData::new(),
            io_processors: HashMap::new(),
        }
    }
}

impl Datamodel for NullDatamodel {
    fn global(&mut self) -> &mut GlobalData {
        &mut self.global
    }

    fn global_s(&self) -> &GlobalData {
        &self.global
    }

    fn get_name(self: &Self) -> &str {
        return NULL_DATAMODEL;
    }

    #[allow(non_snake_case)]
    fn initializeDataModel(self: &mut Self, _fsm: &mut Fsm, _dataState: StateId) {}

    fn set(self: &mut NullDatamodel, _name: &str, _data: Box<dyn Data>) {
        // nothing to do
    }

    fn get(self: &NullDatamodel, _name: &str) -> Option<&dyn Data> {
        None
    }

    fn get_io_processors(&mut self) -> &mut HashMap<String, Box<dyn EventIOProcessor>> {
        return &mut self.io_processors;
    }

    fn get_mut<'v>(&'v mut self, _name: &str) -> Option<&'v mut dyn Data> {
        None
    }

    fn clear(self: &mut NullDatamodel) {}

    fn log(self: &mut NullDatamodel, msg: &str) {
        println!("Log: {}", msg);
    }

    fn execute(&mut self, _fsm: &Fsm, _script: &str) -> String {
        "".to_string()
    }

    fn execute_for_each(&mut self, _fsm: &Fsm, _array_expression: &str, _item: &str, _index: &str,
                        _execute_body: &mut dyn FnMut(&mut dyn Datamodel)) {
        // nothing to do
    }

    /// #W3C says:
    /// The boolean expression language consists of the In predicate only.
    /// It has the form 'In(id)', where id is the id of a state in the enclosing state machine.
    /// The predicate must return 'true' if and only if that state is in the current state configuration.
    fn execute_condition(&mut self, _fsm: &Fsm, _script: &str) -> Result<bool, String> {
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

pub trait Data: ToAny + Send + Debug + ToString {
    fn get_copy(&self) -> Box<dyn Data>;
    fn is_numeric(&self) -> bool {
        false
    }
    fn as_number(&self) -> f64 {
        0.0
    }
}

pub fn get_data_as<T: 'static>(ec: &mut dyn Data) -> Option<&mut T> {
    let va = ec.as_any();
    match va.downcast_mut::<T>() {
        Some(v) => {
            Some(v)
        }
        None => {
            None
        }
    }
}

pub struct StringData {
    pub value: String,
}

impl StringData {
    pub fn new(val: &str) -> StringData {
        StringData {
            value: val.to_string(),
        }
    }
}

impl Debug for StringData {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl ToString for StringData {
    fn to_string(&self) -> String {
        self.value.clone()
    }
}

impl Data for StringData {
    fn get_copy(&self) -> Box<dyn Data> {
        Box::new(StringData {
            value: self.value.clone(),
        })
    }
}

#[derive(Debug)]
pub struct FloatData {
    pub value: f64,
}

impl FloatData {
    pub fn new(val: f64) -> FloatData {
        FloatData {
            value: val
        }
    }
}

impl ToString for FloatData {
    fn to_string(&self) -> String {
        self.value.to_string()
    }
}

impl Data for FloatData {
    fn get_copy(&self) -> Box<dyn Data> {
        Box::new(FloatData { value: self.value })
    }

    fn is_numeric(&self) -> bool {
        true
    }

    fn as_number(&self) -> f64 {
        self.value
    }
}


#[derive(Debug)]
pub struct EmptyData {}

impl EmptyData {
    pub fn new() -> EmptyData {
        EmptyData {}
    }
}

impl ToString for EmptyData {
    fn to_string(&self) -> String {
        "".to_string()
    }
}

impl Data for EmptyData {
    fn get_copy(&self) -> Box<dyn Data> {
        Box::new(EmptyData {})
    }
}

#[derive(Debug)]
pub struct DataStore {
    pub values: HashMap<String, Box<dyn Data>>,

}

impl DataStore {
    pub fn new() -> DataStore {
        DataStore {
            values: HashMap::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&Box<dyn Data>> {
        if self.values.contains_key(key) {
            self.values.get(key)
        } else {
            None
        }
    }

    pub fn get_mut<'v>(&'v mut self, key: &str) -> Option<&'v mut Box<dyn Data>> {
        if self.values.contains_key(key) {
            self.values.get_mut(key)
        } else {
            None
        }
    }

    pub fn set(&mut self, key: &str, data: Box<dyn Data>) {
        self.values.insert(key.to_string(), data);
    }
}