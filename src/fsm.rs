//! Implements the W3C recommendation data-structures and algorithms described in the W3C scxml recommendation.\
//! As reference each type and method has the w3c description as documentation.\
//! See [W3C:Algorithm for SCXML Interpretation](/doc/W3C_SCXML_2024_07_13/index.html#AlgorithmforSCXMLInterpretation)

#![allow(non_camel_case_types)]
#![allow(clippy::doc_lazy_continuation)]

use lazy_static::lazy_static;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::{Debug, Display, Formatter};
use std::hash::Hash;
use std::ops::DerefMut;
#[cfg(test)]
use std::println as error;
#[cfg(test)]
#[cfg(feature = "Debug")]
use std::println as debug;
use std::slice::Iter;
use std::str::FromStr;
use std::string::ToString;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::{fmt, thread};

#[cfg(not(test))]
use log::error;

use crate::actions::{Action, ActionWrapper};
#[cfg(not(test))]
#[cfg(feature = "Debug")]
use log::debug;
use timer::Guard;

use crate::datamodel::{Data, DataArc, DataStore, Datamodel, DatamodelFactory, GlobalDataArc, NullDatamodelFactory, NULL_DATAMODEL, NULL_DATAMODEL_LC, SCXML_INVOKE_TYPE, SCXML_INVOKE_TYPE_SHORT, SESSION_ID_VARIABLE_NAME, SESSION_NAME_VARIABLE_NAME, create_data_arc};
#[cfg(feature = "ECMAScript")]
use crate::ecma_script_datamodel::ECMAScriptDatamodelFactory;
#[cfg(feature = "ECMAScript")]
use crate::ecma_script_datamodel::ECMA_SCRIPT_LC;

use crate::event_io_processor::EventIOProcessor;
use crate::executable_content::ExecutableContent;

#[cfg(feature = "RfsmExpressionModel")]
use crate::expression_engine::datamodel::{RFsmExpressionDatamodelFactory, RFSM_EXPRESSION_DATAMODEL_LC};

use crate::fsm::BindingType::{Early, Late};
use crate::fsm_executor::FsmExecutor;
use crate::get_global;
use crate::scxml_event_io_processor::{SCXML_EVENT_PROCESSOR_SHORT_TYPE, SCXML_TARGET_SESSION_ID_PREFIX};

#[cfg(feature = "Trace")]
use crate::tracer::create_tracer;
#[cfg(feature = "Trace")]
use crate::tracer::{TraceMode, Tracer};

/// Platform specific event to cancel the current session.
pub const EVENT_CANCEL_SESSION: &str = "error.platform.cancel";
pub const EVENT_DONE_INVOKE_PREFIX: &str = "done.invoke.";

pub static PLATFORM_ID_COUNTER: AtomicU32 = AtomicU32::new(1);

pub static THREAD_ID_COUNTER: AtomicU32 = AtomicU32::new(1);

/// Starts the FSM inside a worker thread.
///
pub fn start_fsm(sm: Box<Fsm>, actions: ActionWrapper, executor: Box<FsmExecutor>) -> ScxmlSession {
    start_fsm_with_data(sm, actions, executor, &Vec::new())
}

pub fn start_fsm_with_data(
    sm: Box<Fsm>,
    actions: ActionWrapper,
    executor: Box<FsmExecutor>,
    data: &[ParamPair],
) -> ScxmlSession {
    start_fsm_with_data_and_finish_mode(sm, actions, executor, data, FinishMode::DISPOSE)
}

pub fn start_fsm_with_data_and_finish_mode(
    mut sm: Box<Fsm>,
    actions: ActionWrapper,
    executor: Box<FsmExecutor>,
    data: &[ParamPair],
    finish_mode: FinishMode,
) -> ScxmlSession {
    #![allow(non_snake_case)]
    let externalQueue: BlockingQueue<Box<Event>> = BlockingQueue::new();
    let sender = externalQueue.sender.clone();

    let data_copy = data.to_vec();
    let session_id: SessionId = SESSION_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut session = ScxmlSession::new_without_join_handle(session_id, sender.clone());

    match finish_mode {
        FinishMode::DISPOSE => {}
        FinishMode::KEEP_CONFIGURATION => {
            // FSM shall enter the final configuration during exct.
            let _ = session
                .global_data
                .lock()
                .unwrap()
                .final_configuration
                .insert(Vec::new());
        }
        FinishMode::NOTHING => {}
    }

    let options = {
        let mut execute_state = executor.state.lock().unwrap();

        execute_state.sessions.insert(session_id, session.clone());

        execute_state.datamodel_options.clone()
    };

    let global_data = session.global_data.clone();
    {
        let mut gc = global_data.lock().unwrap();
        gc.actions = actions;
        let executor_state_lock = executor.state.lock();
        let guard = executor_state_lock.unwrap();
        for p in &guard.processors {
            let pg = p.lock().unwrap();
            for t in pg.get_types() {
                gc.io_processors.insert(t.to_string(), p.clone());
            }
        }
    }

    let thread = thread::Builder::new()
        .name(format!(
            "fsm_{}",
            THREAD_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
        .spawn(move || {
            #[cfg(feature = "Debug")]
            debug!("SM starting...");
            {
                let mut datamodel = create_datamodel(sm.datamodel.as_str(), global_data, &options);
                {
                    let mut global = get_global!(datamodel);
                    global.externalQueue = externalQueue;
                    global.session_id = session_id;
                    global.caller_invoke_id = Option::map(sm.caller_invoke_id.as_ref(), |x| x.clone());
                    global.parent_session_id = sm.parent_session_id;
                    global.executor = Some(executor);

                    // W3C:
                    // If the value of a key ... matches the 'id' of a <data> element
                    // in the top-level data model of the invoked session, the SCXML Processor
                    // MUST use the value of the key as the initial value of the corresponding
                    // <data> element.
                    if !data_copy.is_empty() {
                        let root_state = sm.get_state_by_id_mut(sm.pseudo_root);
                        for val in data_copy {
                            if root_state.data.get_mut(&val.name).is_some() {
                                root_state.data.insert(val.name, create_data_arc(val.value.clone()));
                            }
                        }
                    }
                }
                sm.interpret(datamodel.deref_mut());
            }
            #[cfg(feature = "Debug")]
            debug!("SM finished");
        });

    let _ = session.thread.insert(thread.unwrap());
    session
}

////////////////////////////////////////////////////////////////////////////////
// ## General Purpose Data types
// Structs and methods are designed to match the signatures in the W3c-Pseudo-code.

/// ## General Purpose List type, as used in the W3C algorithm.
#[derive(Clone, Default)]
pub struct List<T: Clone> {
    data: Vec<T>,
}

impl<T: Clone + PartialEq> List<T> {
    pub fn new() -> List<T> {
        List {
            data: Default::default(),
        }
    }

    /// Extension to create a list from an array.
    pub fn from_array(l: &[T]) -> List<T> {
        List { data: l.to_vec() }
    }

    /// Extension to return the current size of the list.
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// Extension to add an element at the end of the list.
    pub fn push(&mut self, t: T) {
        self.data.push(t);
    }

    /// Extension to merge the specified list into this list.
    pub fn push_set(&mut self, l: &OrderedSet<T>) {
        for i in l.data.iter() {
            self.data.push((*i).clone());
        }
    }

    /// *W3C says:* Returns the head of the list
    pub fn head(&self) -> &T {
        self.data.first().unwrap()
    }

    /// *W3C says*:
    /// Returns the tail of the list (i.e., the rest of the list once the head is removed)
    pub fn tail(&self) -> List<T> {
        let mut t = List {
            data: self.data.clone(),
        };
        t.data.remove(0);
        t
    }

    /// *W3C says*:
    /// Returns the list appended with l
    pub fn append(&self, l: &List<T>) -> List<T> {
        let mut t = List {
            data: self.data.clone(),
        };
        for i in l.data.iter() {
            t.data.push((*i).clone());
        }
        t
    }

    /// *W3C says*:
    /// Returns the list appended with l
    pub fn append_set(&self, l: &OrderedSet<T>) -> List<T> {
        let mut t = List {
            data: self.data.clone(),
        };
        for i in l.data.iter() {
            t.data.push((*i).clone());
        }
        t
    }

    /// *W3C says*:
    /// Returns the list of elements that satisfy the predicate f
    /// # Actual Implementation:
    /// Can't name the function "filter" because this get in conflict with pre-defined "filter"
    /// that is introduced by the Iterator-implementation.
    pub fn filter_by(&self, f: &dyn Fn(&T) -> bool) -> List<T> {
        let mut t = List::new();

        for i in self.data.iter() {
            if f(i) {
                t.data.push((*i).clone());
            }
        }
        t
    }

    /// *W3C says*:
    /// Returns true if some element in the list satisfies the predicate f.  Returns false for an empty list.
    pub fn some(&self, f: &dyn Fn(&T) -> bool) -> bool {
        for si in &self.data {
            if f(si) {
                return true;
            }
        }
        false
    }

    /// *W3C says*:
    /// Returns true if every element in the list satisfies the predicate f.  Returns true for an empty list.
    pub fn every(&self, f: &dyn Fn(&T) -> bool) -> bool {
        for si in &self.data {
            if !f(si) {
                return false;
            }
        }
        true
    }

    /// Returns a sorted copy of the list.
    pub fn sort<F>(&self, compare: &F) -> List<T>
    where
        F: Fn(&T, &T) -> std::cmp::Ordering + ?Sized,
    {
        let mut t = List {
            data: self.data.clone(),
        };
        t.data.sort_by(compare);
        t
    }

    /// Extension to support "for in" semantics.
    pub fn iterator(&self) -> Iter<'_, T> {
        self.data.iter()
    }

    /// Extension to support conversion to ordered sets.\
    /// Returns a new OrderedSet with copies of the elements in this list.
    /// Duplicates are removed.
    pub fn to_set(&self) -> OrderedSet<T> {
        let mut s = OrderedSet::new();
        for e in self.data.iter() {
            s.add(e.clone());
        }
        s
    }

    /// Returns the last element as mutable reference.
    pub fn last_mut(&mut self) -> &mut T {
        self.data.last_mut().unwrap()
    }
}

/// Set datatype used by the algorithm,
/// *W3C says*:
/// Note that the algorithm assumes a Lisp-like semantics in which the empty Set null is equivalent
/// to boolean 'false' and all other entities are equivalent to 'true'.
///
/// The notation \[...] is used as a list constructor, so that '\[t]' denotes a list whose only member
/// is the object t.
#[derive(Debug, Clone, Default)]
pub struct OrderedSet<T> {
    pub(crate) data: Vec<T>,
}

impl<T: Clone + PartialEq> OrderedSet<T> {
    pub fn new() -> OrderedSet<T> {
        OrderedSet {
            data: Default::default(),
        }
    }

    pub fn from_array(l: &[T]) -> OrderedSet<T> {
        OrderedSet { data: l.to_vec() }
    }

    /// Extension: The size (only informational)
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// *W3C says*:
    /// Adds e to the set if it is not already a member
    pub fn add(&mut self, e: T) {
        if !self.data.contains(&e) {
            self.data.push(e.clone());
        }
    }

    /// *W3C says*:
    /// Deletes e from the set
    pub fn delete(&mut self, e: &T) {
        self.data.retain(|x| *x != *e);
    }

    /// *W3C says*:
    /// Adds all members of s that are not already members of the set
    /// (s must also be an OrderedSet)
    pub fn union(&mut self, s: &OrderedSet<T>) {
        for si in &s.data {
            if !self.isMember(si) {
                self.add(si.clone());
            }
        }
    }

    /// *W3C says*:
    /// Is e a member of set?
    #[allow(non_snake_case)]
    pub fn isMember(&self, e: &T) -> bool {
        self.data.contains(e)
    }

    /// *W3C says*:
    /// Returns true if some element in the set satisfies the predicate f.
    ///
    /// Returns false for an empty set.
    pub fn some(&self, f: &dyn Fn(&T) -> bool) -> bool {
        for si in &self.data {
            if f(si) {
                return true;
            }
        }
        false
    }

    /// *W3C says*:
    /// Returns true if every element in the set satisfies the predicate f.
    ///
    /// Returns true for an empty set.
    pub fn every(&self, f: &dyn Fn(&T) -> bool) -> bool {
        for si in &self.data {
            if !f(si) {
                return false;
            }
        }
        true
    }

    /// *W3C says*:
    /// Returns true if this set and set s have at least one member in common
    #[allow(non_snake_case)]
    pub fn hasIntersection(&self, s: &OrderedSet<T>) -> bool {
        for si in &self.data {
            if s.isMember(si) {
                return true;
            }
        }
        false
    }

    /// *W3C says*:
    /// Is the set empty?
    #[allow(non_snake_case)]
    pub fn isEmpty(&self) -> bool {
        self.size() == 0
    }

    /// *W3C says*:
    /// Remove all elements from the set (make it empty)
    pub fn clear(&mut self) {
        self.data.clear();
    }

    /// *W3C says*:
    /// Converts the set to a list that reflects the order in which elements were originally added.
    ///
    /// In the case of sets created by intersection, the order of the first set (the one on which
    /// the method was called) is used
    ///
    /// In the case of sets created by union, the members of the first set (the one on which union
    /// was called) retain their original ordering while any members belonging to the second set only
    /// are placed after, retaining their ordering in their original set.
    #[allow(non_snake_case)]
    pub fn toList(&self) -> List<T> {
        let mut l = List::new();
        for e in self.data.iter() {
            l.push(e.clone());
        }
        l
    }

    pub fn sort<F>(&self, compare: &F) -> List<T>
    where
        F: Fn(&T, &T) -> std::cmp::Ordering + ?Sized,
    {
        let mut t = List {
            data: self.data.clone(),
        };
        t.data.sort_by(compare);
        t
    }

    pub fn iterator(&self) -> Iter<'_, T> {
        self.data.iter()
    }
}

/// Queue datatype used by the algorithm
#[derive(Debug, Default)]
pub struct Queue<T> {
    data: VecDeque<T>,
}

impl<T> Queue<T> {
    fn new() -> Queue<T> {
        Queue {
            data: VecDeque::new(),
        }
    }

    /// Extension to re-use exiting instances.
    pub fn clear(&mut self) {
        self.data.clear();
    }

    /// *W3C says*:
    /// Puts e last in the queue
    pub fn enqueue(&mut self, e: T) {
        self.data.push_back(e);
    }

    /// *W3C says*:
    /// Removes and returns first element in queue
    pub fn dequeue(&mut self) -> T {
        self.data.pop_front().unwrap()
    }

    /// *W3C says*:
    /// Is the queue empty?
    #[allow(non_snake_case)]
    pub fn isEmpty(&self) -> bool {
        self.data.is_empty()
    }
}

#[derive(Debug)]
pub struct BlockingQueue<T> {
    pub sender: Sender<T>,
    pub receiver: Arc<Mutex<Receiver<T>>>,
}

impl<T> Default for BlockingQueue<T> {
    fn default() -> Self {
        BlockingQueue::new()
    }
}

impl<T> BlockingQueue<T> {
    fn new() -> BlockingQueue<T> {
        let (sender, receiver) = channel();
        BlockingQueue {
            receiver: Arc::new(Mutex::new(receiver)),
            sender,
        }
    }

    /// *W3C says*:
    /// Puts e last in the queue
    pub fn enqueue(&mut self, e: T) {
        let _ = self.sender.send(e);
    }

    /// *W3C says*:
    /// Removes and returns first element in queue, blocks if queue is empty
    pub fn dequeue(&mut self) -> T {
        self.receiver.lock().unwrap().recv().unwrap()
    }
}

/// *W3C says*:
/// table\[foo] returns the value associated with foo.
/// table\[foo] = bar sets the value associated with foo to be bar.
/// #Actual implementation:
/// Instead of the Operators, methods are used.
#[derive(Debug, Default)]
pub struct HashTable<K, T> {
    data: HashMap<K, T>,
}

impl<K: Eq + Hash + Clone, T: Clone> HashTable<K, T> {
    fn new() -> HashTable<K, T> {
        HashTable {
            data: HashMap::new(),
        }
    }
    /// Extension to re-use exiting instances.
    pub fn clear(&mut self) {
        self.data.clear();
    }

    pub fn put(&mut self, k: K, v: &T) {
        self.data.insert(k.clone(), v.clone());
    }

    pub fn put_move(&mut self, k: K, v: T) {
        self.data.insert(k.clone(), v);
    }

    pub fn put_all(&mut self, t: &HashTable<K, T>) {
        for (k, v) in &t.data {
            self.data.insert(k.clone(), v.clone());
        }
    }

    pub fn has(&self, k: K) -> bool {
        self.data.contains_key(&k)
    }

    pub fn get(&self, k: K) -> &T {
        self.data.get(&k).unwrap()
    }
}

/////////////////////////////////////////////////////////////
// FSM model (State etc, representing the XML-data-model)

pub type Name = String;
pub type StateId = u32;
pub type DocumentId = u32;
pub type ExecutableContentId = u32;
pub type StateVec = Vec<State>;
pub type StateNameMap = HashMap<Name, StateId>;
pub type TransitionMap = HashMap<TransitionId, Transition>;

/// Datamodel binding type. See [W3C SCXML Data Binding](/doc/W3C_SCXML_2024_07_13/index.html#DataBinding)
#[derive(Debug, Clone, PartialEq, Copy, Default)]
pub enum BindingType {
    #[default]
    Early,
    Late,
}

impl BindingType {
    pub fn from_ordinal(ordinal: u8) -> BindingType {
        match ordinal {
            1 => Early,
            2 => Late,
            _ => {
                panic!("Unknown ordinal {} for BindingType", ordinal);
            }
        }
    }

    pub fn ordinal(&self) -> u8 {
        match self {
            Early => 1u8,
            Late => 2u8,
        }
    }
}

impl FromStr for BindingType {
    type Err = ();

    fn from_str(input: &str) -> Result<BindingType, Self::Err> {
        match input.to_lowercase().as_str() {
            "early" => Ok(BindingType::Early),
            "late" => Ok(BindingType::Late),
            _ => Err(()),
        }
    }
}

/// Event type.
#[derive(Debug, Clone, PartialEq, Copy, Default)]
pub enum EventType {
    /// for events raised by the platform itself, such as error events
    platform,
    /// for events raised by \<raise\> and \<send\> with target '_internal'
    internal,

    #[default]
    /// for all other events
    external,
}

impl EventType {
    pub fn name(&self) -> &'static str {
        match self {
            EventType::platform => "platform",
            EventType::internal => "internal",
            EventType::external => "external",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParamPair {
    pub name: String,
    pub value: Data,
}

impl ParamPair {
    pub fn new_moved(name: String, value: Data) -> ParamPair {
        ParamPair { name, value }
    }
    pub fn new(name: &str, value: &Data) -> ParamPair {
        ParamPair {
            name: name.to_string(),
            value: value.clone(),
        }
    }
}

/// *W3C says*:
/// ##The Internal Structure of Events.
/// Events have an internal structure which is reflected in the _event variable. This variable can be
/// accessed to condition transitions (via boolean expressions in the 'cond' attribute) or to update
/// the data model (via \<assign\>), etc.
///
/// The SCXML Processor must ensure that the following fields are present in all events, whether
/// internal or external.
///
/// - name. This is a character string giving the name of the event. The SCXML Processor must set
///   the name field to the name of this event. It is what is matched against the 'event' attribute
///   of \<transition\>. Note that transitions can do additional tests by using the value of this
///   field inside boolean expressions in the 'cond' attribute.
/// - type. This field describes the event type. The SCXML Processor must set it to: "platform"
///   (for events raised by the platform itself, such as error events), "internal" (for events
///   raised by \<raise\> and \<send\> with target '_internal') or "external" (for all other events).
/// - sendid. If the sending entity has specified a value for this, the Processor must set this
///   field to that value (see C Event I/O Processors for details). Otherwise, in the case of error
///   events triggered by a failed attempt to send an event, the Processor must set this field to
///   the send id of the triggering \<send\> element. Otherwise it must leave it blank.
/// - origin. This is a URI, equivalent to the 'target' attribute on the \<send\> element. For
///   external events, the SCXML Processor should set this field to a value which, when used as the
///   value of 'target', will allow the receiver of the event to \<send\> a response back to the
///   originating entity via the Event I/O Processor specified in 'origintype'. For internal and
///   platform events, the Processor must leave this field blank.
/// - origintype. This is equivalent to the 'type' field on the \<send\> element. For external events,
///   the SCXML Processor should set this field to a value which, when used as the value of 'type',
///   will allow the receiver of the event to \<send\> a response back to the originating entity at
///   the URI specified by 'origin'. For internal and platform events, the Processor must leave this
///   field blank.
/// - invokeid. If this event is generated from an invoked child process, the SCXML Processor must
///   set this field to the invoke id of the invocation that triggered the child process. Otherwise
///   it must leave it blank.
/// - data. This field contains whatever data the sending entity chose to include in this event.
///   The receiving SCXML Processor should reformat this data to match its data model, but must not
///   otherwise modify it. If the conversion is not possible, the Processor must leave the field
///   blank and must place an error 'error.execution' in the internal event queue.
///
#[derive(Debug, Clone, Default)]
pub struct Event {
    pub name: String,
    pub etype: EventType,
    pub sendid: Option<String>,
    pub origin: Option<String>,
    pub origin_type: Option<String>,
    pub invoke_id: Option<InvokeId>,

    /// Name-Value pairs from \<param\> elements.
    pub param_values: Option<Vec<ParamPair>>,

    /// Content from \<content\> element.
    pub content: Option<DataArc>,
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl Event {
    pub fn new_simple(name: &str) -> Event {
        Event {
            name: name.to_string(),
            etype: EventType::external,
            sendid: None,
            origin: None,
            param_values: None,
            content: None,
            invoke_id: None,
            origin_type: None,
        }
    }

    pub fn new(
        prefix: &str,
        id: &str,
        data_params: Option<Vec<ParamPair>>,
        data_content: Option<DataArc>,
        event_type: EventType,
    ) -> Event {
        Event {
            name: format!("{}{}", prefix, id),
            etype: event_type,
            sendid: None,
            origin: None,
            param_values: data_params,
            content: data_content,
            invoke_id: None,
            origin_type: None,
        }
    }

    #[cfg(feature = "Trace")]
    pub fn trace(t: TraceMode, enable: bool) -> Event {
        Event {
            name: format!("trace.{}.{}", t, if enable { "on" } else { "off" }),
            etype: EventType::external,
            sendid: None,
            origin: None,
            param_values: None,
            content: None,
            invoke_id: None,
            origin_type: None,
        }
    }

    pub fn error(name: &str) -> Event {
        Event {
            name: format!("error.{}", name),
            etype: EventType::platform,
            sendid: None,
            origin: None,
            param_values: None,
            content: None,
            invoke_id: None,
            origin_type: None,
        }
    }

    /// W3C: Indicates that an error internal to the execution of the document has occurred, such as one arising from expression evaluation.
    pub fn error_execution_with_event(event: &Event) -> Event {
        Event {
            name: "error.execution".to_string(),
            etype: EventType::platform,
            sendid: event.sendid.clone(),
            origin: event.origin.clone(),
            param_values: None,
            content: None,
            invoke_id: event.invoke_id.clone(),
            origin_type: event.origin_type.clone(),
        }
    }

    /// W3C: Indicates that an error internal to the execution of the document has occurred, such as one arising from expression evaluation.
    pub fn error_execution(send_id: &Option<String>, invoke_id: &Option<InvokeId>) -> Event {
        Event {
            name: "error.execution".to_string(),
            etype: EventType::platform,
            sendid: send_id.clone(),
            origin: None,
            param_values: None,
            content: None,
            invoke_id: invoke_id.clone(),
            origin_type: None,
        }
    }

    /// W3C: Indicates that an error has occurred while trying to communicate with an external entity.
    pub fn error_communication(event: &Event) -> Event {
        Event {
            name: "error.communication".to_string(),
            etype: EventType::platform,
            sendid: event.sendid.clone(),
            origin: event.origin.clone(),
            param_values: None,
            content: None,
            invoke_id: event.invoke_id.clone(),
            origin_type: event.origin_type.clone(),
        }
    }

    pub fn get_copy(&self) -> Box<Event> {
        Box::new(Event {
            invoke_id: self.invoke_id.clone(),
            param_values: self.param_values.clone(),
            content: self.content.clone(),
            name: self.name.clone(),
            etype: self.etype,
            sendid: self.sendid.clone(),
            origin: self.origin.clone(),
            origin_type: self.origin_type.clone(),
        })
    }
}

pub type InvokeId = String;

pub type EventSender = Sender<Box<Event>>;

#[derive(Clone, PartialEq, Debug, Default)]
pub struct CommonContent {
    /// content inside \<content\> child
    pub content: Option<String>,

    /// expr-attribute of \<content\> child
    pub content_expr: Option<String>,
}

impl CommonContent {
    pub fn new() -> CommonContent {
        CommonContent {
            content: None,
            content_expr: None,
        }
    }
}

type OptionalParams = Option<Vec<Parameter>>;

pub fn push_param(params: &mut OptionalParams, param: Parameter) {
    if params.is_none() {
        params.insert(Vec::new()).push(param);
    } else {
        params.as_mut().unwrap().push(param);
    }
}

#[derive(Clone, PartialEq, Default)]
/// *W3C says*:
/// The \<invoke\> element is used to create an instance of an external service.
pub struct Invoke {
    pub doc_id: DocumentId,

    /// *W3C says*:
    /// Attribute 'idlocation':\
    /// Location expression.\
    /// Any data model expression evaluating to a data model location.\
    /// Must not occur with the 'id' attribute.
    pub external_id_location: String,

    /// *W3C says*:
    /// Attribute 'type':\
    /// A URI specifying the type of the external service.
    pub type_name: Data,

    /// *W3C says*:
    /// Attribute 'typeexpr':\
    /// A dynamic alternative to 'type'. If this attribute is present, the SCXML Processor must evaluate it
    /// when the parent \<invoke\> element is evaluated and treat the result as if it had been entered as
    /// the value of 'type'.
    pub type_expr: Data,

    /// *W3C says*:
    /// List of valid location expressions
    pub name_list: Vec<String>,

    /// *W3C says*:
    /// A URI to be passed to the external service.\
    /// Must not occur with the 'srcexpr' attribute or the \<content\> element.
    pub src: Data,

    /// *W3C says*:
    /// A dynamic alternative to 'src'. If this attribute is present,
    /// the SCXML Processor must evaluate it when the parent \<invoke\> element is evaluated and treat the result
    /// as if it had been entered as the value of 'src'.
    pub src_expr: Data,

    /// *W3C says*:
    /// Boolean.\
    /// A flag indicating whether to forward events to the invoked process.
    pub autoforward: bool,

    /// *W3C says*:
    /// Executable content to handle the data returned from the invoked component.
    /// Occurs 0 or 1 times. See 6.5 \<finalize}> for details.
    pub finalize: ExecutableContentId,

    /// Generated invokeId (identical to "id" if specified).
    pub invoke_id: String,

    pub parent_state_name: String,

    /// \<param\> children
    pub params: Option<Vec<Parameter>>,

    pub content: Option<CommonContent>,
}

impl Invoke {
    pub fn new() -> Invoke {
        Invoke {
            doc_id: 0,
            invoke_id: "".to_string(),
            parent_state_name: "".to_string(),
            external_id_location: "".to_string(),
            type_name: Data::None(),
            type_expr: Data::None(),
            name_list: vec![],
            src: Data::None(),
            src_expr: Data::None(),
            autoforward: false,
            params: None,
            content: None,
            finalize: 0,
        }
    }
}

impl Debug for Invoke {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Invoke")
            .field("invokeId", &self.invoke_id)
            .field("idlocation", &self.external_id_location)
            .field("type", &self.type_name)
            .field("typeexpr", &self.type_expr)
            .field("src", &self.src)
            .field("srcexpr", &self.src_expr)
            .field("autoforward", &self.autoforward)
            .field("params", &self.params)
            .field("content", &self.content)
            .finish()
    }
}

/// Stores \<param\> elements for \<send\>, \<donedata\> or \<invoke\>
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Parameter {
    pub name: String,
    pub expr: String,
    pub location: String,
}

/// *W3C says*:
/// ##Global variables
/// The following variables are global from the point of view of the algorithm.
/// Their values will be set in the procedure interpret().
/// #Actual Implementation
/// In the W3C algorithm the datamodel is simple a global variable.
/// As the datamodel needs access to other global variables and rust doesn't like
/// accessing data of parents from inside a member, most global data is moved to
/// this struct that is owned by the datamodel.
#[allow(non_snake_case)]
#[derive(Default)]
pub struct GlobalData {
    pub executor: Option<Box<FsmExecutor>>,
    pub actions: ActionWrapper,
    pub configuration: OrderedSet<StateId>,
    pub statesToInvoke: OrderedSet<StateId>,
    pub historyValue: HashTable<StateId, OrderedSet<StateId>>,
    pub running: bool,

    internalQueue: Queue<Event>,

    pub externalQueue: BlockingQueue<Box<Event>>,

    /// Invoked Sessions. Key: InvokeId.
    pub child_sessions: HashMap<InvokeId, ScxmlSession>,

    /// Set if this FSM was created as result of some invoke.
    pub caller_invoke_id: Option<InvokeId>,
    pub parent_session_id: Option<SessionId>,

    /// Unique Id of the owning session.
    pub session_id: SessionId,

    /// Will contain after execution the final configuration, if set before.
    pub final_configuration: Option<Vec<String>>,
    pub environment: HashMap<String, DataArc>,

    /// Stores any delayed send (with a "sendid"), Key: sendid
    pub delayed_send: HashMap<String, Guard>,
    pub io_processors: HashMap<String, Arc<Mutex<Box<dyn EventIOProcessor>>>>,

    pub data: DataStore,
}

impl GlobalData {
    pub fn new() -> GlobalData {
        GlobalData {
            executor: None,
            actions: ActionWrapper::new(),
            configuration: OrderedSet::new(),
            historyValue: HashTable::new(),
            running: false,
            statesToInvoke: OrderedSet::new(),
            internalQueue: Queue::new(),
            externalQueue: BlockingQueue::new(),
            child_sessions: HashMap::new(),
            caller_invoke_id: None,
            parent_session_id: None,
            session_id: 0,
            final_configuration: None,
            environment: HashMap::new(),
            delayed_send: HashMap::new(),
            io_processors: HashMap::new(),
            data: DataStore::new(),
        }
    }

    pub fn enqueue_internal(&mut self, event: Event) {
        self.internalQueue.enqueue(event);
    }
}

/// Mode how the executor handles the ScxmlSession
/// if the FSM is finished.
#[derive(Debug, Clone)]
pub enum FinishMode {
    DISPOSE,
    KEEP_CONFIGURATION,
    NOTHING,
}

/// Represents some external session.
/// Holds thread-id and channel-sender to the external queue of the session.
pub struct ScxmlSession {
    pub session_id: SessionId,
    pub thread: Option<JoinHandle<()>>,
    pub sender: Sender<Box<Event>>,
    /// global_data should be access after the FSM is finished to avoid deadlocks.
    pub global_data: GlobalDataArc,
    /// Doc-id of the Invoke element that triggered this session.
    /// InvokeIds are generated if not specified, to identify the invoke element, the doc-id
    /// is used.
    pub invoke_doc_id: DocumentId,
    /// State of the invoke or 0.
    pub state_id: Option<StateId>,
}

impl Debug for ScxmlSession {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScxmlSession")
            .field("session_id", &self.session_id)
            .field("inv_doc_id", &self.invoke_doc_id)
            .field("state_id", &self.state_id)
            .finish()
    }
}

impl ScxmlSession {
    pub fn new_without_join_handle(id: SessionId, sender: Sender<Box<Event>>) -> ScxmlSession {
        ScxmlSession {
            session_id: id,
            thread: None,
            sender,
            global_data: GlobalDataArc::new(Mutex::new(GlobalData::new())),
            invoke_doc_id: 0,
            state_id: None,
        }
    }
}

impl Clone for ScxmlSession {
    fn clone(&self) -> Self {
        ScxmlSession {
            session_id: self.session_id,
            thread: None,
            sender: self.sender.clone(),
            global_data: self.global_data.clone(),
            state_id: self.state_id,
            invoke_doc_id: self.invoke_doc_id,
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.session_id = source.session_id;
        self.thread = None;
        self.sender = source.sender.clone();
        self.state_id = source.state_id;
        self.invoke_doc_id = source.invoke_doc_id;
    }
}

pub type SessionId = u32;

/// The FSM implementation, according to W3C proposal.
#[allow(non_snake_case)]
pub struct Fsm {
    #[cfg(feature = "Trace")]
    pub tracer: Box<dyn Tracer>,
    pub datamodel: String,
    pub binding: BindingType,
    pub version: String,
    pub statesNames: StateNameMap,
    pub executableContent: HashMap<ExecutableContentId, Vec<Box<dyn ExecutableContent>>>,

    pub name: String,

    /// An FSM can have actual multiple initial-target-states, so this state may be artificial.
    /// Reader has to generate a parent state if needed.
    /// This state also serve as the "scxml" state element were mentioned.
    pub pseudo_root: StateId,

    /// The only real storage of states, identified by the Id - the zero based index
    /// into the vector.
    /// If a state has no declared id, one is generated.
    pub states: Vec<State>,
    pub transitions: TransitionMap,

    pub script: ExecutableContentId,

    /// Set if this FSM was created as result of some invoke.
    /// See also Global.caller_invoke_id
    pub caller_invoke_id: Option<InvokeId>,
    pub parent_session_id: Option<SessionId>,

    pub timer: timer::Timer,

    pub generate_id_count: u32,
}

impl Default for Fsm {
    fn default() -> Self {
        Fsm::new()
    }
}

impl Debug for Fsm {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Fsm{{v:{} root:{} states:",
            self.version, self.pseudo_root
        )?;
        display_state_map(&self.states, f)?;
        display_transition_map(&self.transitions, f)?;
        write!(f, "}}")
    }
}

fn display_state_map(sm: &StateVec, f: &mut Formatter<'_>) -> fmt::Result {
    write!(f, "{{")?;

    let mut first = true;
    for e in sm {
        if first {
            first = false;
        } else {
            write!(f, ",")?;
        }
        write!(f, "{}", *e)?;
    }

    write!(f, "}}")
}

fn display_transition_map(sm: &TransitionMap, f: &mut Formatter<'_>) -> fmt::Result {
    write!(f, "{{")?;

    let mut first = true;
    for e in sm {
        if first {
            first = false;
        } else {
            write!(f, ",")?;
        }
        write!(f, "{}", *e.1)?;
    }

    write!(f, "}}")
}

impl Fsm {
    pub fn new() -> Fsm {
        Fsm {
            datamodel: NULL_DATAMODEL.to_string(),
            states: Vec::new(),
            transitions: HashMap::new(),
            pseudo_root: 0,
            #[cfg(feature = "Trace")]
            tracer: create_tracer(),
            caller_invoke_id: None,
            parent_session_id: None,
            name: "FSM".to_string(),
            script: 0,
            version: "1.0".to_string(),
            binding: BindingType::Early,
            statesNames: StateNameMap::new(),
            executableContent: HashMap::new(),
            timer: timer::Timer::new(),
            generate_id_count: 0,
        }
    }

    pub fn get_state_by_name(&self, name: &Name) -> &State {
        self.get_state_by_id(*self.statesNames.get(name).unwrap())
    }

    pub fn get_state_by_name_mut(&mut self, name: &Name) -> &mut State {
        self.get_state_by_id_mut(*self.statesNames.get(name).unwrap())
    }

    /// Gets a state by id.
    /// The id MUST exist.
    pub fn get_state_by_id(&self, state_id: StateId) -> &State {
        self.states.get((state_id - 1) as usize).unwrap()
    }

    /// Gets a mutable state by id.
    /// The id MUST exist.
    pub fn get_state_by_id_mut(&mut self, state_id: StateId) -> &mut State {
        self.states.get_mut((state_id - 1) as usize).unwrap()
    }

    pub fn get_transition_by_id_mut(&mut self, transition_id: TransitionId) -> &mut Transition {
        self.transitions.get_mut(&transition_id).unwrap()
    }

    pub fn get_transition_by_id(&self, transition_id: TransitionId) -> &Transition {
        self.transitions.get(&transition_id).unwrap()
    }

    fn state_document_order(&self, sid1: &StateId, sid2: &StateId) -> std::cmp::Ordering {
        // TODO: Optimize! Do that state-ids == index in fsm.states.
        let s1 = self.get_state_by_id(*sid1);
        let s2 = self.get_state_by_id(*sid2);

        match s1.doc_id {
            di if di > s2.doc_id => std::cmp::Ordering::Greater,
            di if di == s2.doc_id => std::cmp::Ordering::Equal,
            _ => std::cmp::Ordering::Less,
        }
    }

    fn state_entry_order(&self, s1: &StateId, s2: &StateId) -> std::cmp::Ordering {
        // Same as Document order
        self.state_document_order(s1, s2)
    }

    fn state_exit_order(&self, s1: &StateId, s2: &StateId) -> std::cmp::Ordering {
        // Reverse Document order
        self.state_document_order(s2, s1)
    }

    fn transition_document_order(&self, t1: &&Transition, t2: &&Transition) -> std::cmp::Ordering {
        match t1.doc_id {
            ti if ti > t2.doc_id => std::cmp::Ordering::Greater,
            ti if ti == t2.doc_id => std::cmp::Ordering::Equal,
            _ => std::cmp::Ordering::Less,
        }
    }

    fn invoke_document_order(s1: &Invoke, s2: &Invoke) -> std::cmp::Ordering {
        match s1.doc_id {
            si if si > s2.doc_id => std::cmp::Ordering::Greater,
            si if si == s2.doc_id => std::cmp::Ordering::Equal,
            _ => std::cmp::Ordering::Less,
        }
    }

    /// Implements variant "initializeDataModel(datamodel, doc)" from W3C.
    fn initialize_data_models_recursive(&mut self, datamodel: &mut dyn Datamodel, state_id: StateId, set_data: bool) {
        datamodel.initializeDataModel(self, state_id, set_data);

        for child_state in self.getChildStates(state_id).iterator() {
            self.initialize_data_models_recursive(datamodel, *child_state, set_data);
        }
    }

    /// *W3C says*:
    /// The purpose of this procedure is to initialize the interpreter and to start processing.
    ///
    /// In order to interpret an SCXML document, first (optionally) perform
    /// [xinclude](/doc/W3C_SCXML_2024_07_13/index.html#xinclude) processing and (optionally) validate
    /// the document, throwing an exception if validation fails.
    /// Then convert initial attributes to \<initial\> container children with transitions
    /// to the state specified by the attribute. (This step is done purely to simplify the statement of
    /// the algorithm and has no effect on the system's behavior.
    ///
    /// Such transitions will not contain any executable content).
    /// Initialize the global data structures, including the data model.
    /// If binding is set to 'early', initialize the data model.
    /// Then execute the global \<script\> element, if any.
    /// Finally, call enterStates on the initial configuration, set the global running
    /// variable to true and start the interpreter's event loop.
    /// ```ignore
    /// procedure interpret(doc):
    ///     if not valid(doc): failWithError()
    ///     expandScxmlSource(doc)
    ///     configuration = new OrderedSet()
    ///     statesToInvoke = new OrderedSet()
    ///     internalQueue = new Queue()
    ///     externalQueue = new BlockingQueue()
    ///     historyValue = new HashTable()
    ///     datamodel = new Datamodel(doc)
    ///     if doc.binding == "early":
    ///         initializeDatamodel(datamodel, doc)
    ///     running = true
    ///     executeGlobalScriptElement(doc)
    ///     enterStates([doc.initial.transition])
    ///     mainEventLoop()
    /// ```
    pub fn interpret(&mut self, datamodel: &mut dyn Datamodel) {
        #[cfg(feature = "Trace_Method")]
        self.tracer.enter_method("interpret");
        if !self.valid() {
            self.failWithError();
            return;
        }
        self.expandScxmlSource();
        {
            datamodel.clear();

            // Initialize session variables "_name" and "_sessionid"

            let session_id = datamodel.global_s().lock().unwrap().session_id;
            datamodel.initialize_read_only(SESSION_ID_VARIABLE_NAME, Data::Integer(session_id as i64));
            // TODO :Escape name
            datamodel.initialize_read_only(SESSION_NAME_VARIABLE_NAME, Data::String(self.name.clone()));

            {
                let mut gd = get_global!(datamodel);
                gd.internalQueue.clear();
                gd.historyValue.clear();
                gd.running = true;
            }

            datamodel.add_functions(self);
            datamodel.set_ioprocessors();

            self.initialize_data_models_recursive(
                datamodel,
                self.pseudo_root,
                self.binding == BindingType::Early,
            );
        }
        self.executeGlobalScriptElement(datamodel);

        let mut inital_states = List::new();
        let itid = self.get_state_by_id(self.pseudo_root).initial;
        if itid != 0 {
            inital_states.push(itid);
        }
        self.enterStates(datamodel, &inital_states);
        self.mainEventLoop(datamodel);
        #[cfg(feature = "Trace_Method")]
        self.tracer.exit_method("interpret");
    }

    /// #Actual implementation:
    /// TODO
    /// * check if all state/transition references are correct (all states have a document-id)
    /// * check if all special scxml conditions are satisfied.
    fn valid(&self) -> bool {
        for state in &self.states {
            if state.doc_id == 0 {
                #[cfg(feature = "Trace")]
                self.tracer
                    .trace(format!("Referenced state '{}' is not declared", state.name).as_str());
                return false;
            }
        }
        true
    }

    /// #Actual implementation:
    /// Throws a panic
    #[allow(non_snake_case)]
    fn failWithError(&self) {
        error!("FSM has failed");
    }

    /// #Actual implementation:
    /// This method is called on the fsm model, after
    /// the xml document was processed. It should check if all References to states are fulfilled.
    /// After this method all "StateId" or "TransactionId" shall be valid and have to lead to a panic.
    #[allow(non_snake_case)]
    fn expandScxmlSource(&mut self) {}

    #[allow(non_snake_case)]
    fn executeGlobalScriptElement(&mut self, datamodel: &mut dyn Datamodel) {
        #[cfg(feature = "Trace_Method")]
        self.tracer.enter_method("executeGlobalScriptElement");
        if self.script != 0 {
            datamodel.executeContent(self, self.script);
        }
        #[cfg(feature = "Trace_Method")]
        self.tracer.exit_method("executeGlobalScriptElement");
    }

    /// *W3C says*:
    /// ## procedure mainEventLoop()
    /// This loop runs until we enter a top-level final state or an external entity cancels processing.
    /// In either case 'running' will be set to false (see EnterStates, below, for termination by
    /// entering a top-level final state.)
    ///
    /// At the top of the loop, we have either just entered the state machine, or we have just
    /// processed an external event. Each iteration through the loop consists of four main steps:
    /// 1) Complete the macrostep by repeatedly taking any internally enabled transitions, namely
    /// those that don't require an event or that are triggered by an internal event.
    /// After each such transition/microstep, check to see if we have reached a final state. 2) When
    /// there are no more internally enabled transitions available, the macrostep is done. Execute
    /// any \<invoke\> tags for states that we entered on the last iteration through the loop 3) If
    /// any internal events have been generated by the invokes, repeat step 1 to handle any errors
    /// raised by the \<invoke\> elements. 4) When the internal event queue is empty, wait for an
    /// external event and then execute any transitions that it triggers. However special preliminary
    /// processing is applied to the event if the state has executed any \<invoke\> elements. First,
    /// if this event was generated by an invoked process, apply \<finalize\> processing to it.
    /// Secondly, if any \<invoke\> elements have autoforwarding set, forward the event to them.
    /// These steps apply before the transitions are taken.
    ///
    /// This event loop thus enforces run-to-completion semantics, in which the system process an external event and then takes all the 'follow-up' transitions that the processing has enabled before looking for another external event. For example, suppose that the external event queue contains events ext1 and ext2 and the machine is in state s1. If processing ext1 takes the machine to s2 and generates internal event int1, and s2 contains a transition t triggered by int1, the system is guaranteed to take t, no matter what transitions s2 or other states have that would be triggered by ext2. Note that this is true even though ext2 was already in the external event queue when int1 was generated. In effect, the algorithm treats the processing of int1 as finishing up the processing of ext1.
    /// ```ignore
    /// procedure mainEventLoop():
    ///     while running:
    ///         enabledTransitions = null
    ///         macrostepDone = false
    ///         # Here we handle eventless transitions and transitions
    ///         # triggered by internal events until macrostep is complete
    ///         while running and not macrostepDone:
    ///             enabledTransitions = selectEventlessTransitions()
    ///             if enabledTransitions.isEmpty():
    ///                 if internalQueue.isEmpty():
    ///                     macrostepDone = true
    ///                 else:
    ///                     internalEvent = internalQueue.dequeue()
    ///                     datamodel["_event"] = internalEvent
    ///                     enabledTransitions = selectTransitions(internalEvent)
    ///             if not enabledTransitions.isEmpty():
    ///                 microstep(enabledTransitions.toList())
    ///         # either we're in a final state, and we break out; of the loop
    ///         if not running:
    ///             break
    ///         # or; we've completed a macrostep, so we start a new macrostep by waiting for an external event
    ///         # Here we invoke whatever needs to be invoked. The implementation of 'invoke' is platform-specific
    ///         for state in statesToInvoke.sort(entryOrder):
    ///             for inv in state.invoke.sort(documentOrder):
    ///                 invoke(inv)
    ///         statesToInvoke.clear()
    ///         # Invoking may have raised internal error events and we iterate to handle them
    ///         if not internalQueue.isEmpty():
    ///             continue;
    ///         # A blocking wait for an external event.  Alternatively, if we have been invoked
    ///         # our parent session also might cancel us.  The mechanism for this is platform specific,
    ///         # but here we assume it’s a special event we receive
    ///         externalEvent = externalQueue.dequeue()
    ///         if isCancelEvent(externalEvent):
    ///             running = false
    ///             continue;
    ///         datamodel["_event"] = externalEvent
    ///         for state in configuration:
    ///             for inv in state.invoke:
    ///                 if inv.invokeid == externalEvent.invokeid:
    ///                     applyFinalize(inv, externalEvent)
    ///                 if inv.autoforward:
    ///                     send(inv.id, externalEvent)
    ///         enabledTransitions = selectTransitions(externalEvent)
    ///         if not enabledTransitions.isEmpty():
    ///             microstep(enabledTransitions.toList())
    ///     # End of outer while running loop.  If we get here, we have reached a top-level final state or have been cancelled
    ///     exitInterpreter()
    /// ```
    #[allow(non_snake_case)]
    fn mainEventLoop(&mut self, datamodel: &mut dyn Datamodel) {
        #[cfg(feature = "Trace_Method")]
        self.tracer.enter_method("mainEventLoop");

        let caller_invoke_id = {
            match &self.caller_invoke_id {
                None => "".to_string(),
                Some(cid) => cid.clone(),
            }
        };

        while get_global!(datamodel).running {
            let mut enabledTransitions;
            let mut macrostepDone = false;
            // Here we handle eventless transitions and transitions
            // triggered by internal events until macrostep is complete
            while get_global!(datamodel).running && !macrostepDone {
                enabledTransitions = self.selectEventlessTransitions(datamodel);
                if enabledTransitions.isEmpty() {
                    if get_global!(datamodel).internalQueue.isEmpty() {
                        macrostepDone = true;
                    } else {
                        #[cfg(feature = "Trace_Method")]
                        self.tracer.enter_method("internalQueue.dequeue");

                        let internalEvent = { get_global!(datamodel).internalQueue.dequeue() };
                        #[cfg(feature = "Trace_Method")]
                        self.tracer.exit_method("internalQueue.dequeue");
                        #[cfg(feature = "Trace_Event")]
                        self.tracer.event_internal_received(&internalEvent);
                        // TODO: Optimize it, set event only once
                        datamodel.set_event(&internalEvent);
                        enabledTransitions = self.selectTransitions(datamodel, &internalEvent);
                    }
                }
                if !enabledTransitions.isEmpty() {
                    self.microstep(datamodel, &enabledTransitions.toList())
                }
            }
            // either we're in a final state, and we break out of the loop
            if !get_global!(datamodel).running {
                break;
            }
            // or we've completed a macrostep, so we start a new macrostep by waiting for an external event
            // Here we invoke whatever needs to be invoked. The implementation of 'invoke' is platform-specific
            let sortedStatesToInvoke = get_global!(datamodel)
                .statesToInvoke
                .sort(&|s1, s2| self.state_entry_order(s1, s2));
            for sid in sortedStatesToInvoke.iterator() {
                let state = self.get_state_by_id(*sid);
                for inv in state.invoke.sort(&Fsm::invoke_document_order).iterator() {
                    self.invoke(datamodel, *sid, inv);
                }
            }

            let externalEvent;
            {
                let externalQueue_receiver = {
                    let mut global_lock = get_global!(datamodel);

                    // let gdb = datamodel.global();
                    global_lock.statesToInvoke.clear();
                    // Invoking may have raised internal error events and we iterate to handle them
                    if !global_lock.internalQueue.isEmpty() {
                        continue;
                    }
                    global_lock.externalQueue.receiver.clone()
                };

                // W3C says:
                //   A blocking wait for an external event.  Alternatively, if we have been invoked
                //   our parent session also might cancel us.  The mechanism for this is platform specific,
                //   but here we assume it’s a special event we receive
                #[cfg(feature = "Trace_Method")]
                self.tracer.enter_method("externalQueue.dequeue");
                loop {
                    let externalEventTmp = externalQueue_receiver.lock().unwrap().recv().unwrap();
                    if externalEventTmp.name.starts_with(EVENT_DONE_INVOKE_PREFIX) {
                        externalEvent = externalEventTmp;
                        break;
                    }
                    if let Some(invoke_id) = &externalEventTmp.invoke_id {
                        if caller_invoke_id.ne(invoke_id) {
                            // W3C says:
                            //    Once it cancels the invoked session, the Processor MUST ignore any events
                            //    it receives from that session. In particular it MUST NOT not insert them
                            //    into the external event queue of the invoking session.
                            // Check if the session is active.
                            if get_global!(datamodel)
                                .child_sessions
                                .contains_key(invoke_id)
                            {
                                externalEvent = externalEventTmp;
                                break;
                            } else {
                                #[cfg(feature = "Debug")]
                                debug!(
                                    "Ignore event {} from invoke {}",
                                    externalEventTmp.name, invoke_id
                                );
                            }
                        } else {
                            externalEvent = externalEventTmp;
                            break;
                        }
                    } else {
                        externalEvent = externalEventTmp;
                        break;
                    }
                }
                #[cfg(feature = "Trace_Method")]
                self.tracer.exit_method("externalQueue.dequeue");
                #[cfg(feature = "Trace_Event")]
                self.tracer.event_external_received(&externalEvent);
                if self.isCancelEvent(&externalEvent) {
                    get_global!(datamodel).running = false;
                    continue;
                }

                if externalEvent.name.starts_with(EVENT_DONE_INVOKE_PREFIX) {
                    if let Some(invoke_id) = &externalEvent.invoke_id {
                        get_global!(datamodel).child_sessions.remove(invoke_id);
                    }
                }
            }
            let mut toFinalize: Vec<ExecutableContentId> = Vec::new();
            let mut toForward: Vec<InvokeId> = Vec::new();
            {
                match externalEvent.invoke_id {
                    None => {}
                    Some(ref invokeId) => {
                        match get_global!(datamodel).child_sessions.get(invokeId) {
                            None => {}
                            Some(session) => {
                                // Get state of invokeid
                                if let Some(state_id) = session.state_id {
                                    let invoke_doc_id = session.invoke_doc_id;
                                    let state = self.get_state_by_id(state_id);
                                    for inv in state.invoke.iterator() {
                                        if inv.doc_id == invoke_doc_id {
                                            toFinalize.push(inv.finalize);
                                        }
                                        if inv.autoforward {
                                            toForward.push(invokeId.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                };
            }
            datamodel.set_event(&externalEvent);
            for finalizeContentId in toFinalize {
                // applyFinalize
                self.executeContent(datamodel, finalizeContentId);
            }
            for invokeId in toForward {
                // When the 'autoforward' attribute is set to true, the SCXML Processor must send an
                // exact copy of every external event it receives to the invoked process.
                // All the fields specified in 5.10.1 The Internal Structure of Events must have the
                // same values in the forwarded copy of the event. The SCXML Processor must forward
                // the event at the point at which it removes it from the external event queue of
                // the invoking session for processing.
                match get_global!(datamodel).child_sessions.get(&invokeId) {
                    None => {
                        // TODO: Clarify, communication error?
                    }
                    Some(session) => {
                        match session.sender.send(externalEvent.clone()) {
                            Ok(_) => {}
                            Err(_) => {
                                // TODO: Clarify, communication error?
                            }
                        }
                    }
                }
            }

            enabledTransitions = self.selectTransitions(datamodel, &externalEvent);
            if !enabledTransitions.isEmpty() {
                self.microstep(datamodel, &enabledTransitions.toList());
            }
        }
        // End of outer while running loop.  If we get here, we have reached a top-level final state or have been cancelled
        self.exitInterpreter(datamodel);
        #[cfg(feature = "Trace_Method")]
        self.tracer.exit_method("mainEventLoop");
    }

    /// *W3C says*:
    /// # procedure exitInterpreter()
    /// The purpose of this procedure is to exit the current SCXML process by exiting all active
    /// states. If the machine is in a top-level final state, a Done event is generated.
    /// (Note that in this case, the final state will be the only active state.)
    /// The implementation of returnDoneEvent is platform-dependent, but if this session is the
    /// result of an \<invoke\> in another SCXML session, returnDoneEvent will cause the event
    /// done.invoke.\<id\> to be placed in the external event queue of that session, where \<id\> is
    /// the id generated in that session when the \<invoke\> was executed.
    /// ```ignore
    /// procedure exitInterpreter():
    ///     statesToExit = configuration.toList().sort(exitOrder)
    ///     for s in statesToExit:
    ///         for content in s.onexit.sort(documentOrder):
    ///             executeContent(content)
    ///         for inv in s.invoke:
    ///             cancelInvoke(inv)
    ///         configuration.delete(s)
    ///         if isFinalState(s) and isScxmlElement(s.parent):
    ///             returnDoneEvent(s.donedata)
    /// ```
    #[allow(non_snake_case)]
    fn exitInterpreter(&mut self, datamodel: &mut dyn Datamodel) {
        let statesToExit;
        {
            {
                let mut global = get_global!(datamodel);
                if global.final_configuration.is_some() {
                    let mut fc = Vec::new();
                    for sid in global.configuration.iterator() {
                        fc.push(self.get_state_by_id(*sid).name.clone());
                    }
                    let _ = global.final_configuration.insert(fc);
                }
                statesToExit = global
                    .configuration
                    .toList()
                    .sort(&|s1, s2| self.state_exit_order(s1, s2));
            }

            let mut session_id_list = Vec::new();
            for session in get_global!(datamodel).child_sessions.values() {
                session_id_list.push(session.session_id);
            }
            if !session_id_list.is_empty() {
                for session_id in session_id_list {
                    datamodel.send(
                        SCXML_EVENT_PROCESSOR_SHORT_TYPE,
                        &Data::String(format!("{}{}", SCXML_TARGET_SESSION_ID_PREFIX, session_id)),
                        Event::new_simple(EVENT_CANCEL_SESSION),
                    );
                }
            }
        }
        for sid in statesToExit.iterator() {
            let mut content: Vec<ExecutableContentId> = Vec::new();
            {
                let s = self.get_state_by_id(*sid);
                content.extend_from_slice(s.onexit.as_slice());
            }
            for ct in content {
                self.executeContent(datamodel, ct);
            }
            get_global!(datamodel).configuration.delete(sid);
            {
                let s = self.get_state_by_id(*sid);
                if self.isFinalState(s) && self.isSCXMLElement(s.parent) {
                    self.returnDoneEvent(&s.donedata.clone(), datamodel);
                }
            }
        }
    }

    /// *W3C says*:
    /// The implementation of returnDoneEvent is platform-dependent, but if this session is the
    /// result of an \<invoke\> in another SCXML session, returnDoneEvent will cause the event
    /// done.invoke.\<id\> to be placed in the external event queue of that session, where \<id\> is
    /// the id generated in that session when the \<invoke\> was executed.
    #[allow(non_snake_case)]
    fn returnDoneEvent(&mut self, _done_data: &Option<DoneData>, datamodel: &mut dyn Datamodel) {
        let caller_invoke_id;
        let parent_session_id;
        {
            let global = get_global!(datamodel);
            caller_invoke_id = global.caller_invoke_id.clone();
            parent_session_id = global.parent_session_id;
        }
        match parent_session_id {
            None => {
                // No parent
            }
            Some(session_id) => {
                match caller_invoke_id {
                    None => {
                        panic!("Internal Error: Caller-Invoke-Id not available but Parent-Session-Id is set.");
                    }
                    Some(invoke_id) => {
                        // TODO: Evaluate done_data, EventType::external ?
                        let mut event = Event::new(
                            EVENT_DONE_INVOKE_PREFIX,
                            &invoke_id,
                            None,
                            None,
                            EventType::external,
                        );
                        event.invoke_id = Some(invoke_id);
                        datamodel.send(
                            SCXML_EVENT_PROCESSOR_SHORT_TYPE,
                            &Data::String(format!("{}{}", SCXML_TARGET_SESSION_ID_PREFIX, session_id)),
                            event,
                        );
                    }
                }
            }
        }
    }

    /// *W3C says*:
    /// # function selectEventlessTransitions()
    /// This function selects all transitions that are enabled in the current configuration that
    /// do not require an event trigger. First find a transition with no 'event' attribute whose
    /// condition evaluates to true. If multiple matching transitions are present, take the first
    /// in document order. If none are present, search in the state's ancestors in ancestry order
    /// until one is found. As soon as such a transition is found, add it to enabledTransitions,
    /// and proceed to the next atomic state in the configuration. If no such transition is found
    /// in the state or its ancestors, proceed to the next state in the configuration.
    /// When all atomic states have been visited and transitions selected, filter the set of enabled
    /// transitions, removing any that are preempted by other transitions, then return the
    /// resulting set.
    /// ```ignore
    ///
    /// function selectEventlessTransitions():
    ///     enabledTransitions = new OrderedSet()
    ///     atomicStates = configuration.toList().filter(isAtomicState).sort(documentOrder)
    ///     for state in atomicStates:
    ///         loop: for s in [state].append(getProperAncestors(state, null)):
    ///             for t in s.transition.sort(documentOrder):
    ///                 if not t.event and conditionMatch(t):
    ///                     enabledTransitions.add(t)
    ///                     break loop;
    ///     enabledTransitions = removeConflictingTransitions(enabledTransitions)
    ///     return enabledTransitions;
    /// ```
    #[allow(non_snake_case)]
    fn selectEventlessTransitions(&mut self, datamodel: &mut dyn Datamodel) -> OrderedSet<TransitionId> {
        #[cfg(feature = "Trace_Method")]
        self.tracer.enter_method("selectEventlessTransitions");

        let mut enabledTransitions: OrderedSet<TransitionId> = OrderedSet::new();
        let atomicStates = get_global!(datamodel)
            .configuration
            .toList()
            .filter_by(&|sid| -> bool { self.isAtomicState(self.get_state_by_id(*sid)) })
            .sort(&|s1, s2| self.state_document_order(s1, s2));
        #[cfg(feature = "Trace_Method")]
        self.tracer.trace_argument("atomicStates", &atomicStates);
        for sid in atomicStates.iterator() {
            let mut states: List<StateId> = List::new();
            states.push(*sid);
            states.push_set(&self.getProperAncestors(*sid, 0));
            let mut condT = Vec::new();
            for s in states.iterator() {
                let state = self.get_state_by_id(*s);
                for t in self
                    .to_transition_list(&state.transitions)
                    .sort(&|t1: &&Transition, t2: &&Transition| self.transition_document_order(t1, t2))
                    .iterator()
                {
                    if t.events.is_empty() {
                        condT.push(t.id);
                    }
                }
            }
            for ct in condT {
                if self.conditionMatch(datamodel, ct) {
                    enabledTransitions.add(ct);
                    break;
                }
            }
        }
        enabledTransitions = self.removeConflictingTransitions(datamodel, &enabledTransitions);
        #[cfg(feature = "Trace_Method")]
        self.tracer
            .trace_result("enabledTransitions", &enabledTransitions);
        #[cfg(feature = "Trace_Method")]
        self.tracer.exit_method("selectEventlessTransitions");
        enabledTransitions
    }

    /// *W3C says*:
    /// function selectTransitions(event)
    /// The purpose of the selectTransitions()procedure is to collect the transitions that are enabled by this event in the current configuration.
    ///
    /// Create an empty set of enabledTransitions. For each atomic state , find a transition whose 'event' attribute matches event and whose condition evaluates to true. If multiple matching transitions are present, take the first in document order. If none are present, search in the state's ancestors in ancestry order until one is found. As soon as such a transition is found, add it to enabledTransitions, and proceed to the next atomic state in the configuration. If no such transition is found in the state or its ancestors, proceed to the next state in the configuration. When all atomic states have been visited and transitions selected, filter out any preempted transitions and return the resulting set.
    /// ```ignore
    /// function selectTransitions(event):
    ///     enabledTransitions = new OrderedSet()
    ///     atomicStates = configuration.toList().filter(isAtomicState).sort(documentOrder)
    ///     for state in atomicStates:
    ///         loop: for s in [state].append(getProperAncestors(state, null)):
    ///             for t in s.transition.sort(documentOrder):
    ///                 if t.event and nameMatch(t.event, event.name) and conditionMatch(t):
    ///                     enabledTransitions.add(t)
    ///                     break loop;
    ///     enabledTransitions = removeConflictingTransitions(enabledTransitions)
    ///     return enabledTransitions;
    /// ```
    #[allow(non_snake_case)]
    fn selectTransitions(&mut self, datamodel: &mut dyn Datamodel, event: &Event) -> OrderedSet<TransitionId> {
        #[cfg(feature = "Trace_Method")]
        self.tracer.enter_method("selectTransitions");

        let mut enabledTransitions: OrderedSet<TransitionId> = OrderedSet::new();
        let atomicStates = get_global!(datamodel)
            .configuration
            .toList()
            .filter_by(&|sid| -> bool { self.isAtomicStateId(sid) })
            .sort(&|s1, s2| self.state_document_order(s1, s2));
        for state in atomicStates.iterator() {
            let mut condT = Vec::new();
            for sid in List::from_array(&[*state])
                .append_set(&self.getProperAncestors(*state, 0))
                .iterator()
            {
                let s = self.get_state_by_id(*sid);
                let mut transition: Vec<&Transition> = Vec::new();
                for tid in s.transitions.iterator() {
                    transition.push(self.get_transition_by_id(*tid));
                }

                transition.sort_by(&|t1: &&Transition, t2: &&Transition| self.transition_document_order(t1, t2));
                for t in transition {
                    if (!t.events.is_empty()) && t.nameMatch(event.name.as_str()) {
                        condT.push(t.id);
                    }
                }
            }
            for ct in condT {
                if self.conditionMatch(datamodel, ct) {
                    enabledTransitions.add(ct);
                    break;
                }
            }
        }
        enabledTransitions = self.removeConflictingTransitions(datamodel, &enabledTransitions);
        #[cfg(feature = "Trace_Method")]
        self.tracer
            .trace_result("enabledTransitions", &enabledTransitions);
        #[cfg(feature = "Trace_Method")]
        self.tracer.exit_method("selectTransitions");
        enabledTransitions
    }

    /// *W3C says*:
    /// #function removeConflictingTransitions(enabledTransitions)
    /// enabledTransitions will contain multiple transitions only if a parallel state is active.
    /// In that case, we may have one transition selected for each of its children.
    /// These transitions may conflict with each other in the sense that they have incompatible
    /// target states. Loosely speaking, transitions are compatible when each one is contained
    /// within a single \<state\> child of the \<parallel\> element.
    /// Transitions that aren't contained within a single child force the state
    /// machine to leave the \<parallel\> ancestor (even if they reenter it later). Such transitions
    /// conflict with each other, and with transitions that remain within a single <state> child, in that
    /// they may have targets that cannot be simultaneously active. The test that transitions have non-
    /// intersecting exit sets captures this requirement. (If the intersection is null, the source and
    /// targets of the two transitions are contained in separate <state> descendants of \<parallel\>.
    /// If intersection is non-null, then at least one of the transitions is exiting the \<parallel\>).
    /// When such a conflict occurs, then if the source state of one of the transitions is a descendant
    /// of the source state of the other, we select the transition in the descendant. Otherwise we prefer
    /// the transition that was selected by the earlier state in document order and discard the other
    /// transition. Note that targetless transitions have empty exit sets and thus do not conflict with
    /// any other transitions.
    ///
    /// We start with a list of enabledTransitions and produce a conflict-free list of filteredTransitions.
    /// For each t1 in enabledTransitions, we test it against all t2 that are already selected in
    /// filteredTransitions. If there is a conflict, then if t1's source state is a descendant of
    /// t2's source state, we prefer t1 and say that it preempts t2
    /// (so we we make a note to remove t2 from filteredTransitions).
    /// Otherwise, we prefer t2 since it was selected in an earlier state in document order,
    /// so we say that it preempts t1.
    /// (There's no need to do anything in this case since t2 is already in filteredTransitions.
    /// Furthermore, once one transition preempts t1, there is no need to test t1 against any other
    /// transitions.)
    /// Finally, if t1 isn't preempted by any transition in filteredTransitions, remove any
    /// transitions that it preempts and add it to that list.
    /// ```ignore
    /// function removeConflictingTransitions(enabledTransitions):
    ///     filteredTransitions = new OrderedSet()
    ///     //toList sorts the transitions in the order of the states that selected them
    ///     for t1 in enabledTransitions.toList():
    ///         t1Preempted = false
    ///         transitionsToRemove = new OrderedSet()
    ///         for t2 in filteredTransitions.toList():
    ///             if computeExitSet([t1]).hasIntersection(computeExitSet([t2])):
    ///                 if isDescendant(t1.source, t2.source):
    ///                     transitionsToRemove.add(t2)
    ///                 else:
    ///                     t1Preempted = true
    ///                     break
    ///         if not; t1Preempted:
    ///             for t3 in transitionsToRemove.toList():
    ///                 filteredTransitions.delete(t3)
    ///             filteredTransitions.add(t1)
    ///
    ///     return filteredTransitions;
    /// ```
    #[allow(non_snake_case)]
    fn removeConflictingTransitions(
        &self,
        datamodel: &mut dyn Datamodel,
        enabledTransitions: &OrderedSet<TransitionId>,
    ) -> OrderedSet<TransitionId> {
        let mut filteredTransitions: OrderedSet<TransitionId> = OrderedSet::new();
        //toList sorts the transitions in the order of the states that selected them
        for tid1 in enabledTransitions.toList().iterator() {
            let t1 = self.get_transition_by_id(*tid1);
            let mut t1Preempted = false;
            let mut transitionsToRemove = OrderedSet::new();
            let filteredTransitionList = filteredTransitions.toList();
            for tid2 in filteredTransitionList.iterator() {
                if self
                    .computeExitSet(datamodel, &List::from_array(&[*tid1]))
                    .hasIntersection(&self.computeExitSet(datamodel, &List::from_array(&[*tid2])))
                {
                    let t2 = self.get_transition_by_id(*tid2);
                    if self.isDescendant(t1.source, t2.source) {
                        transitionsToRemove.add(tid2);
                    } else {
                        t1Preempted = true;
                        break;
                    }
                }
            }
            if !t1Preempted {
                for t3 in transitionsToRemove.toList().iterator() {
                    filteredTransitions.delete(t3);
                }
                filteredTransitions.add(*tid1);
            }
        }
        filteredTransitions
    }

    /// *W3C says*:
    /// # procedure microstep(enabledTransitions)
    /// The purpose of the microstep procedure is to process a single set of transitions. These may have been enabled by an external event, an internal event, or by the presence or absence of certain values in the data model at the current point in time. The processing of the enabled transitions must be done in parallel ('lock step') in the sense that their source states must first be exited, then their actions must be executed, and finally their target states entered.
    ///
    /// If a single atomic state is active, then enabledTransitions will contain only a single transition. If multiple states are active (i.e., we are in a parallel region), then there may be multiple transitions, one per active atomic state (though some states may not select a transition.) In this case, the transitions are taken in the document order of the atomic states that selected them.
    /// ```ignore
    /// procedure microstep(enabledTransitions):
    ///     exitStates(enabledTransitions)
    ///     executeTransitionContent(enabledTransitions)
    ///     enterStates(enabledTransitions)
    /// ```
    #[allow(non_snake_case)]
    fn microstep(&mut self, datamodel: &mut dyn Datamodel, enabledTransitions: &List<TransitionId>) {
        #[cfg(feature = "Trace_Method")]
        self.tracer.enter_method("microstep");
        #[cfg(feature = "Debug")]
        if enabledTransitions.size() > 0 {
            if enabledTransitions.size() > 1 {
                debug!("Enabled Transitions:");
                for t in enabledTransitions.iterator() {
                    if let Some(transition) = self.transitions.get(t) {
                        debug!("\t{}", transition);
                    }
                }
            } else {
                let t = enabledTransitions.head();
                if let Some(transition) = self.transitions.get(t) {
                    debug!("Enabled Transition {}", transition);
                }
            }
        }
        self.exitStates(datamodel, enabledTransitions);
        self.executeTransitionContent(datamodel, enabledTransitions);
        self.enterStates(datamodel, enabledTransitions);
        #[cfg(feature = "Trace_Method")]
        self.tracer.exit_method("microstep");
    }

    /// *W3C says*:
    /// # procedure exitStates(enabledTransitions)
    /// Compute the set of states to exit. Then remove all the states on statesToExit from the set
    /// of states that will have invoke processing done at the start of the next macrostep.
    /// (Suppose macrostep M1 consists of microsteps m11 and m12. We may enter state s in m11 and
    /// exit it in m12. We will add s to statesToInvoke in m11, and must remove it in m12. In the
    /// subsequent macrostep M2, we will apply invoke processing to all states that were entered,
    /// and not exited, in M1.) Then convert statesToExit to a list and sort it in exitOrder.
    ///
    /// For each state s in the list, if s has a deep history state h, set the history value of h
    /// to be the list of all atomic descendants of s that are members in the current configuration,
    /// else set its value to be the list of all immediate children of s that are members of the
    /// current configuration. Again for each state s in the list, first execute any onexit
    /// handlers, then cancel any ongoing invocations, and finally remove s from the current
    /// configuration.
    ///
    /// ```ignore
    /// procedure exitStates(enabledTransitions):
    ///     statesToExit = computeExitSet(enabledTransitions)
    ///     for s in statesToExit:
    ///         statesToInvoke.delete(s)
    ///     statesToExit = statesToExit.toList().sort(exitOrder)
    ///     for s in statesToExit:
    ///         for h in s.history:
    ///             if h.type == "deep":
    ///                 f = lambda s0: isAtomicState(s0) and isDescendant(s0,s)
    ///             else:
    ///                 f = lambda s0: s0.parent == s
    ///             historyValue[h.id] = configuration.toList().filter(f)
    ///     for s in statesToExit:
    ///         for content in s.onexit.sort(documentOrder):
    ///             executeContent(content)
    ///         for inv in s.invoke:
    ///             cancelInvoke(inv)
    ///         configuration.delete(s)
    /// ```
    #[allow(non_snake_case)]
    fn exitStates(&mut self, datamodel: &mut dyn Datamodel, enabledTransitions: &List<TransitionId>) {
        #[cfg(feature = "Trace_Method")]
        self.tracer.enter_method("exitStates");

        let statesToExit = self.computeExitSet(datamodel, enabledTransitions);

        {
            let mut gd = get_global!(datamodel);

            for s in statesToExit.iterator() {
                gd.statesToInvoke.delete(s);
            }
        }
        let statesToExitSorted = statesToExit.sort(&|s1, s2| self.state_exit_order(s1, s2));
        let mut ahistory: HashTable<StateId, OrderedSet<StateId>> = HashTable::new();

        let configStateList = self.set_to_state_list(&get_global!(datamodel).configuration);

        for sid in statesToExitSorted.iterator() {
            let s = self.get_state_by_id(*sid);
            for hid in s.history.iterator() {
                let h = self.get_state_by_id(*hid);
                if h.history_type == HistoryType::Deep {
                    let stateIdList = self.state_list_to_id_set(
                        &configStateList
                            .filter_by(&|s0| -> bool { self.isAtomicState(s0) && self.isDescendant(s0.id, s.id) }),
                    );
                    ahistory.put_move(h.id, stateIdList);
                } else {
                    let fl = &get_global!(datamodel)
                        .configuration
                        .toList()
                        .filter_by(&|s0| -> bool { self.get_state_by_id(*s0).parent == s.id })
                        .to_set();
                    ahistory.put(h.id, fl);
                }
            }
        }

        get_global!(datamodel).historyValue.put_all(&ahistory);

        for sid in statesToExitSorted.iterator() {
            // Use the document-id of Invoke to identify sessions to cancel.
            let mut invoke_doc_ids = HashSet::new();
            let mut exitList: List<ExecutableContentId> = List::new();
            {
                let s = self.get_state_by_id(*sid);
                #[cfg(feature = "Trace_State")]
                self.tracer.trace_exit_state(s);
                for inv in s.invoke.iterator() {
                    invoke_doc_ids.insert(inv.doc_id);
                }
                for ec in &s.onexit {
                    exitList.push(*ec);
                }
            }
            if !invoke_doc_ids.is_empty() {
                let mut session_ids = Vec::new();
                for (invoke_id, session) in &get_global!(datamodel).child_sessions {
                    if invoke_doc_ids.contains(&session.invoke_doc_id) {
                        session_ids.push((invoke_id.clone(), session.session_id));
                    }
                }
                for (invoke_id, session_id) in &session_ids {
                    self.cancelInvoke(datamodel, invoke_id, *session_id);
                }
            }

            for ec in exitList.iterator() {
                self.executeContent(datamodel, *ec);
            }

            get_global!(datamodel).configuration.delete(sid)
        }
        #[cfg(feature = "Trace_Method")]
        self.tracer.exit_method("exitStates");
    }

    /// *W3C says*:
    /// ## procedure enterStates(enabledTransitions)
    /// First, compute the list of all the states that will be entered as a result of taking the
    /// transitions in enabledTransitions. Add them to statesToInvoke so that invoke processing can
    /// be done at the start of the next macrostep. Convert statesToEnter to a list and sort it in
    /// entryOrder. For each state s in the list, first add s to the current configuration.
    /// Then if we are using late binding, and this is the first time we have entered s, initialize
    /// its data model. Then execute any onentry handlers. If s's initial state is being entered by
    /// default, execute any executable content in the initial transition. If a history state in s
    /// was the target of a transition, and s has not been entered before, execute the content
    /// inside the history state's default transition. Finally, if s is a final state, generate
    /// relevant Done events. If we have reached a top-level final state, set running to false as a
    /// signal to stop processing.
    /// ```ignore
    ///    procedure enterStates(enabledTransitions):
    ///        statesToEnter = new OrderedSet()
    ///        statesForDefaultEntry = new OrderedSet()
    ///        // initialize the temporary table for default content in history states
    ///        defaultHistoryContent = new HashTable()
    ///        computeEntrySet(enabledTransitions, statesToEnter, statesForDefaultEntry, defaultHistoryContent)
    ///        for s in statesToEnter.toList().sort(entryOrder):
    ///           configuration.add(s)
    ///           statesToInvoke.add(s)
    ///           if binding == "late" and s.isFirstEntry:
    ///              initializeDataModel(datamodel.s,doc.s)
    ///              s.isFirstEntry = false
    ///           for content in s.onentry.sort(documentOrder):
    ///              executeContent(content)
    ///           if statesForDefaultEntry.isMember(s):
    ///              executeContent(s.initial.transition)
    ///           if defaultHistoryContent[s.id]:
    ///              executeContent(defaultHistoryContent[s.id])
    ///           if isFinalState(s):
    ///              if isSCXMLElement(s.parent):
    ///                 running = false
    ///              else:
    ///                 parent = s.parent
    ///                 grandparent = parent.parent
    ///                 internalQueue.enqueue(new Event("done.state." + parent.id, s.donedata))
    ///                 if isParallelState(grandparent):
    ///                    if getChildStates(grandparent).every(isInFinalState):
    ///                       internalQueue.enqueue(new Event("done.state." + grandparent.id))
    /// ```
    #[allow(non_snake_case)]
    fn enterStates(&mut self, datamodel: &mut dyn Datamodel, enabledTransitions: &List<StateId>) {
        #[cfg(feature = "Trace_Method")]
        self.tracer.enter_method("enterStates");
        let binding = self.binding;
        let mut statesToEnter = OrderedSet::new();
        let mut statesForDefaultEntry = OrderedSet::new();

        // initialize the temporary table for default content in history states
        let mut defaultHistoryContent: HashTable<StateId, ExecutableContentId> = HashTable::new();
        self.computeEntrySet(
            datamodel,
            enabledTransitions,
            &mut statesToEnter,
            &mut statesForDefaultEntry,
            &mut defaultHistoryContent,
        );
        for s in statesToEnter
            .toList()
            .sort(&|s1, s2| self.state_entry_order(s1, s2))
            .iterator()
        {
            #[cfg(feature = "Trace_State")]
            {
                self.tracer.trace_enter_state(self.get_state_by_id(*s));
            }
            {
                let mut gd = get_global!(datamodel);
                gd.configuration.add(*s);
                gd.statesToInvoke.add(*s);
            }
            let mut to_init: StateId = 0;
            {
                let state_s: &mut State = self.get_state_by_id_mut(*s);
                if binding == BindingType::Late && state_s.isFirstEntry {
                    to_init = *s;
                    state_s.isFirstEntry = false;
                }
            }
            if to_init != 0 {
                datamodel.initializeDataModel(self, to_init, true);
            }
            let mut exe = Vec::new();
            {
                let state_s: &State = self.get_state_by_id(*s);
                exe.extend_from_slice(state_s.onentry.as_slice());
                if statesForDefaultEntry.isMember(s) && state_s.initial > 0 {
                    exe.push(self.get_transition_by_id(state_s.initial).content);
                }
                if defaultHistoryContent.has(*s) {
                    exe.push(*defaultHistoryContent.get(*s));
                }
            }

            for ct in exe {
                if ct > 0 {
                    self.executeContent(datamodel, ct);
                }
            }

            if self.isFinalStateId(*s) {
                let state_s = self.get_state_by_id(*s);
                let parent: StateId = state_s.parent;
                if self.isSCXMLElement(parent) {
                    get_global!(datamodel).running = false;
                } else {
                    let parentS = self.get_state_by_id(parent);
                    let mut name_values = Vec::new();
                    let mut content = None;
                    match &state_s.donedata {
                        None => {}
                        Some(done_data) => {
                            datamodel.evaluate_params(&done_data.params, &mut name_values);
                            content = datamodel.evaluate_content(&done_data.content);
                        }
                    }
                    let param_values = if name_values.is_empty() {
                        None
                    } else {
                        Some(name_values)
                    };

                    self.enqueue_internal(
                        datamodel,
                        // TODO: EventType::external ?
                        Event::new(
                            "done.state.",
                            &parentS.name,
                            param_values,
                            content,
                            EventType::external,
                        ),
                    );
                    let stateParent = self.get_state_by_id(parent);
                    let grandparent: StateId = stateParent.parent;
                    if self.isParallelState(grandparent)
                        && self
                            .getChildStates(grandparent)
                            .every(&|s: &StateId| -> bool { self.isInFinalState(datamodel, *s) })
                    {
                        let grandparentS = self.get_state_by_id(grandparent);
                        self.enqueue_internal(
                            datamodel,
                            // TODO: EventType::external ?
                            Event::new(
                                "done.state.",
                                &grandparentS.name,
                                None,
                                None,
                                EventType::external,
                            ),
                        );
                    }
                }
            }
        }
        #[cfg(feature = "Trace_Method")]
        self.tracer.exit_method("enterStates");
    }

    /// Put an event into the internal queue.
    pub fn enqueue_internal(&mut self, datamodel: &mut dyn Datamodel, event: Event) {
        #[cfg(feature = "Trace_Event")]
        self.tracer.event_internal_send(&event);
        get_global!(datamodel).internalQueue.enqueue(event);
    }

    #[allow(non_snake_case)]
    pub fn executeContent(&mut self, datamodel: &mut dyn Datamodel, contentId: ExecutableContentId) {
        #[cfg(feature = "Trace_Method")]
        {
            self.tracer.enter_method("executeContent");
            self.tracer.trace_argument("contentId", &contentId);
        }
        if contentId != 0 {
            datamodel.executeContent(self, contentId);
        }
        #[cfg(feature = "Trace_Method")]
        self.tracer.exit_method("executeContent");
    }

    #[allow(non_snake_case)]
    pub fn isParallelState(&self, state: StateId) -> bool {
        #[cfg(feature = "Trace_Method")]
        {
            self.tracer.enter_method("isParallelState");
            self.tracer.trace_argument("state", &state);
        }
        let b = state > 0 && self.get_state_by_id(state).is_parallel;
        #[cfg(feature = "Trace_Method")]
        {
            self.tracer.trace_result("parallel", &b.to_string());
            self.tracer.exit_method("isParallelState");
        }
        b
    }

    #[allow(non_snake_case)]
    pub fn isSCXMLElement(&self, state: StateId) -> bool {
        state == self.pseudo_root
    }

    #[allow(non_snake_case)]
    pub fn isFinalState(&self, state: &State) -> bool {
        state.is_final
    }

    #[allow(non_snake_case)]
    pub fn isFinalStateId(&self, state: StateId) -> bool {
        self.isFinalState(self.get_state_by_id(state))
    }

    #[allow(non_snake_case)]
    pub fn isAtomicState(&self, state: &State) -> bool {
        state.states.is_empty()
    }

    #[allow(non_snake_case)]
    pub fn isAtomicStateId(&self, sid: &StateId) -> bool {
        self.isAtomicState(self.get_state_by_id(*sid))
    }

    /// *W3C says*:
    /// # procedure computeExitSet(enabledTransitions)
    /// For each transition t in enabledTransitions, if t is targetless then do nothing, else compute the transition's domain.
    /// (This will be the source state in the case of internal transitions) or the least common compound ancestor
    /// state of the source state and target states of t (in the case of external transitions. Add to the statesToExit
    /// set all states in the configuration that are descendants of the domain.
    /// ```ignore
    /// function computeExitSet(transitions)
    ///     statesToExit = new OrderedSet
    ///     for t in transitions:
    ///         if t.target:
    ///             domain = getTransitionDomain(t)
    ///             for s in configuration:
    ///                 if isDescendant(s,domain):
    ///                     statesToExit.add(s)
    ///     return statesToExit;
    /// ```
    #[allow(non_snake_case)]
    fn computeExitSet(&self, datamodel: &mut dyn Datamodel, transitions: &List<TransitionId>) -> OrderedSet<StateId> {
        #[cfg(feature = "Trace_Method")]
        self.tracer.enter_method("computeExitSet");
        #[cfg(feature = "Trace_Method")]
        self.tracer.trace_argument("transitions", &transitions);
        let mut statesToExit: OrderedSet<StateId> = OrderedSet::new();
        for tid in transitions.iterator() {
            let t = self.get_transition_by_id(*tid);
            if !t.target.is_empty() {
                let domain = self.getTransitionDomain(datamodel, t);
                for s in get_global!(datamodel).configuration.iterator() {
                    if self.isDescendant(*s, domain) {
                        statesToExit.add(*s);
                    }
                }
            }
        }
        #[cfg(feature = "Trace_Method")]
        self.tracer.trace_result("statesToExit", &statesToExit);
        #[cfg(feature = "Trace_Method")]
        self.tracer.exit_method("computeExitSet");
        statesToExit
    }

    /// *W3C says*:
    /// # procedure executeTransitionContent(enabledTransitions)
    /// For each transition in the list of enabledTransitions, execute its executable content.
    /// ```ignore
    /// procedure executeTransitionContent(enabledTransitions):
    ///     for t in enabledTransitions:
    ///         executeContent(t)
    /// ```
    #[allow(non_snake_case)]
    fn executeTransitionContent(&mut self, datamodel: &mut dyn Datamodel, enabledTransitions: &List<TransitionId>) {
        for tid in enabledTransitions.iterator() {
            let t = self.get_transition_by_id(*tid);
            if t.content > 0 {
                self.executeContent(datamodel, t.content);
            }
        }
    }

    /// *W3C says*:
    /// # procedure computeEntrySet(transitions, statesToEnter, statesForDefaultEntry, defaultHistoryContent)
    /// Compute the complete set of states that will be entered as a result of taking 'transitions'.
    /// This value will be returned in 'statesToEnter' (which is modified by this procedure). Also
    /// place in 'statesForDefaultEntry' the set of all states whose default initial states were
    /// entered. First gather up all the target states in 'transitions'. Then add them and, for all
    /// that are not atomic states, add all of their (default) descendants until we reach one or
    /// more atomic states. Then add any ancestors that will be entered within the domain of the
    /// transition. (Ancestors outside of the domain of the transition will not have been exited.)
    /// ```ignore
    /// procedure computeEntrySet(transitions, statesToEnter, statesForDefaultEntry, defaultHistoryContent)
    ///     for t in transitions:
    ///         for s in t.target:
    ///             addDescendantStatesToEnter(s,statesToEnter,statesForDefaultEntry, defaultHistoryContent)
    ///         ancestor = getTransitionDomain(t)
    ///         for s in getEffectiveTargetStates(t):
    ///             addAncestorStatesToEnter(s, ancestor, statesToEnter, statesForDefaultEntry, defaultHistoryContent)
    /// ```
    #[allow(non_snake_case)]
    fn computeEntrySet(
        &mut self,
        datamodel: &mut dyn Datamodel,
        transitions: &List<TransitionId>,
        statesToEnter: &mut OrderedSet<StateId>,
        statesForDefaultEntry: &mut OrderedSet<StateId>,
        defaultHistoryContent: &mut HashTable<StateId, ExecutableContentId>,
    ) {
        #[cfg(feature = "Trace_Method")]
        self.tracer.enter_method("computeEntrySet");
        #[cfg(feature = "Trace_Method")]
        self.tracer.trace_argument("transitions", transitions);

        for tid in transitions.iterator() {
            let t = self.get_transition_by_id(*tid);
            for s in t.target.iter() {
                self.addDescendantStatesToEnter(
                    datamodel,
                    *s,
                    statesToEnter,
                    statesForDefaultEntry,
                    defaultHistoryContent,
                );
            }
            let ancestor = self.getTransitionDomain(datamodel, t);
            for s in self.getEffectiveTargetStates(datamodel, t).iterator() {
                self.addAncestorStatesToEnter(
                    datamodel,
                    *s,
                    ancestor,
                    statesToEnter,
                    statesForDefaultEntry,
                    defaultHistoryContent,
                );
            }
        }
        #[cfg(feature = "Trace_Method")]
        self.tracer.trace_result("statesToEnter>", statesToEnter);
        #[cfg(feature = "Trace_Method")]
        self.tracer.exit_method("computeEntrySet");
    }

    /// *W3C says*:
    /// #procedure addDescendantStatesToEnter(state,statesToEnter,statesForDefaultEntry, defaultHistoryContent)
    /// The purpose of this procedure is to add to statesToEnter 'state' and any of its descendants
    /// that the state machine will end up entering when it enters 'state'. (N.B. If 'state' is a
    /// history pseudo-state, we dereference it and add the history value instead.) Note that this '
    /// procedure permanently modifies both statesToEnter and statesForDefaultEntry.
    ///
    /// First, If state is a history state then add either the history values associated with state or state's default
    /// target to statesToEnter. Then (since the history value may not be an immediate descendant of 'state's parent)
    /// add any ancestors between the history value and state's parent. Else (if state is not a history state),
    /// add state to statesToEnter. Then if state is a compound state, add state to statesForDefaultEntry and
    /// recursively call addStatesToEnter on its default initial state(s). Then, since the default initial states
    /// may not be children of 'state', add any ancestors between the default initial states and 'state'.
    /// Otherwise, if state is a parallel state, recursively call addStatesToEnter on any of its child states that
    /// don't already have a descendant on statesToEnter.
    /// ```ignore
    /// procedure addDescendantStatesToEnter(state,statesToEnter,statesForDefaultEntry, defaultHistoryContent):
    ///     if isHistoryState(state):
    ///         if historyValue[state.id]:
    ///             for s in historyValue[state.id]:
    ///                 addDescendantStatesToEnter(s,statesToEnter,statesForDefaultEntry, defaultHistoryContent)
    ///             for s in historyValue[state.id]:
    ///                 addAncestorStatesToEnter(s, state.parent, statesToEnter, statesForDefaultEntry, defaultHistoryContent)
    ///         else:
    ///             defaultHistoryContent[state.parent.id] = state.transition.content
    ///             for s in state.transition.target:
    ///                 addDescendantStatesToEnter(s,statesToEnter,statesForDefaultEntry, defaultHistoryContent)
    ///             for s in state.transition.target:
    ///                 addAncestorStatesToEnter(s, state.parent, statesToEnter, statesForDefaultEntry, defaultHistoryContent)
    ///     else:
    ///         statesToEnter.add(state)
    ///         if isCompoundState(state):
    ///             statesForDefaultEntry.add(state)
    ///             for s in state.initial.transition.target:
    ///                 addDescendantStatesToEnter(s,statesToEnter,statesForDefaultEntry, defaultHistoryContent)
    ///             for s in state.initial.transition.target:
    ///                 addAncestorStatesToEnter(s, state, statesToEnter, statesForDefaultEntry, defaultHistoryContent)
    ///         else:
    ///             if isParallelState(state):
    ///                 for child in getChildStates(state):
    ///                     if not statesToEnter.some(lambda s: isDescendant(s,child)):
    ///                         addDescendantStatesToEnter(child,statesToEnter,statesForDefaultEntry, defaultHistoryContent)
    /// ```
    #[allow(non_snake_case)]
    fn addDescendantStatesToEnter(
        &self,
        datamodel: &mut dyn Datamodel,
        sid: StateId,
        statesToEnter: &mut OrderedSet<StateId>,
        statesForDefaultEntry: &mut OrderedSet<StateId>,
        defaultHistoryContent: &mut HashTable<StateId, ExecutableContentId>,
    ) {
        #[cfg(feature = "Trace_Method")]
        self.tracer.enter_method("addDescendantStatesToEnter");
        #[cfg(feature = "Trace_Method")]
        self.tracer.trace_argument("State", &sid);

        let state = self.get_state_by_id(sid);
        if self.isHistoryState(sid) {
            if get_global!(datamodel).historyValue.has(sid) {
                let mut stateIds: Vec<StateId> = Vec::new();
                for s in get_global!(datamodel).historyValue.get(sid).iterator() {
                    stateIds.push(*s);
                }
                for s in &stateIds {
                    self.addDescendantStatesToEnter(
                        datamodel,
                        *s,
                        statesToEnter,
                        statesForDefaultEntry,
                        defaultHistoryContent,
                    );
                }
                for s in &stateIds {
                    self.addAncestorStatesToEnter(
                        datamodel,
                        *s,
                        state.parent,
                        statesToEnter,
                        statesForDefaultEntry,
                        defaultHistoryContent,
                    );
                }
            } else {
                // A history state have exactly one transition which specified the default history configuration.
                let defaultTransition = self.get_transition_by_id(*state.transitions.head());
                defaultHistoryContent.put(state.parent, &defaultTransition.content);
                for s in &defaultTransition.target {
                    self.addDescendantStatesToEnter(
                        datamodel,
                        *s,
                        statesToEnter,
                        statesForDefaultEntry,
                        defaultHistoryContent,
                    );
                }
                for s in &defaultTransition.target {
                    self.addAncestorStatesToEnter(
                        datamodel,
                        *s,
                        state.parent,
                        statesToEnter,
                        statesForDefaultEntry,
                        defaultHistoryContent,
                    );
                }
            }
        } else {
            statesToEnter.add(sid);
            if self.isCompoundState(sid) {
                statesForDefaultEntry.add(sid);
                if state.initial != 0 {
                    let initialTransition = self.get_transition_by_id(state.initial);
                    for s in &initialTransition.target {
                        self.addDescendantStatesToEnter(
                            datamodel,
                            *s,
                            statesToEnter,
                            statesForDefaultEntry,
                            defaultHistoryContent,
                        );
                    }
                    for s in &initialTransition.target {
                        self.addAncestorStatesToEnter(
                            datamodel,
                            *s,
                            sid,
                            statesToEnter,
                            statesForDefaultEntry,
                            defaultHistoryContent,
                        )
                    }
                }
            } else if self.isParallelState(sid) {
                for child in self.getChildStates(sid).iterator() {
                    if !statesToEnter.some(&|s| self.isDescendant(*s, *child)) {
                        self.addDescendantStatesToEnter(
                            datamodel,
                            *child,
                            statesToEnter,
                            statesForDefaultEntry,
                            defaultHistoryContent,
                        )
                    }
                }
            }
        }
        #[cfg(feature = "Trace_Method")]
        self.tracer.trace_result("statesToEnter", &statesToEnter);
        #[cfg(feature = "Trace_Method")]
        self.tracer.exit_method("addDescendantStatesToEnter");
    }

    /// *W3C says*:
    /// # procedure addAncestorStatesToEnter(state, ancestor, statesToEnter, statesForDefaultEntry, defaultHistoryContent)
    /// Add to statesToEnter any ancestors of 'state' up to, but not including, 'ancestor' that must be entered in order to enter 'state'. If any of these ancestor states is a parallel state, we must fill in its descendants as well.
    /// ```ignore
    /// procedure addAncestorStatesToEnter(state, ancestor, statesToEnter, statesForDefaultEntry, defaultHistoryContent)
    ///     for anc in getProperAncestors(state,ancestor):
    ///         statesToEnter.add(anc)
    ///         if isParallelState(anc):
    ///             for child in getChildStates(anc):
    ///                 if not statesToEnter.some(lambda s: isDescendant(s,child)):
    ///                     addDescendantStatesToEnter(child,statesToEnter,statesForDefaultEntry, defaultHistoryContent)
    /// ```
    #[allow(non_snake_case)]
    fn addAncestorStatesToEnter(
        &self,
        datamodel: &mut dyn Datamodel,
        state: StateId,
        ancestor: StateId,
        statesToEnter: &mut OrderedSet<StateId>,
        statesForDefaultEntry: &mut OrderedSet<StateId>,
        defaultHistoryContent: &mut HashTable<StateId, ExecutableContentId>,
    ) {
        #[cfg(feature = "Trace_Method")]
        self.tracer.enter_method("addAncestorStatesToEnter");
        #[cfg(feature = "Trace_Method")]
        self.tracer.trace_argument("state", &state);
        for anc in self.getProperAncestors(state, ancestor).iterator() {
            statesToEnter.add(*anc);
            if self.isParallelState(*anc) {
                for child in self.getChildStates(*anc).iterator() {
                    if !statesToEnter.some(&|s| self.isDescendant(*s, *child)) {
                        self.addDescendantStatesToEnter(
                            datamodel,
                            *child,
                            statesToEnter,
                            statesForDefaultEntry,
                            defaultHistoryContent,
                        );
                    }
                }
            }
        }
        #[cfg(feature = "Trace_Method")]
        self.tracer.exit_method("addAncestorStatesToEnter");
    }

    /// *W3C says*:
    /// # procedure isInFinalState(s)
    /// Return true if s is a compound \<state\> and one of its children is an active <final> state
    /// (i.e. is a member of the current configuration), or if s is a \<parallel\> state and
    /// isInFinalState is true of all its children.
    /// ```ignore
    /// function isInFinalState(s):
    ///     if isCompoundState(s):
    ///         return getChildStates(s).some(lambda s: isFinalState(s) and configuration.isMember(s));
    ///     elif isParallelState(s):
    ///         return getChildStates(s).every(isInFinalState);
    ///     else:
    ///         return false;
    /// ```
    #[allow(non_snake_case)]
    fn isInFinalState(&self, datamodel: &dyn Datamodel, s: StateId) -> bool {
        if self.isCompoundState(s) {
            self.getChildStates(s).some(&|cs: &StateId| -> bool {
                self.isFinalStateId(*cs)
                    && datamodel
                        .global_s()
                        .lock()
                        .unwrap()
                        .configuration
                        .isMember(cs)
            })
        } else if self.isParallelState(s) {
            self.getChildStates(s)
                .every(&|cs: &StateId| -> bool { self.isInFinalState(datamodel, *cs) })
        } else {
            false
        }
    }

    /// *W3C says*:
    /// # function getTransitionDomain(transition)
    /// Return the compound state such that
    /// 1) all states that are exited or entered as a result of taking 'transition'
    ///    are descendants of it
    /// 2) no descendant of it has this property.
    /// ```ignore
    /// function getTransitionDomain(t)
    ///     tstates = getEffectiveTargetStates(t)
    ///     if not tstates:
    ///         return null;
    ///     elif t.type == "internal" and isCompoundState(t.source) and tstates.every(lambda s: isDescendant(s,t.source)):
    ///         return t.source;
    ///     else:
    ///         return findLCCA([t.source].append(tstates));
    /// ```
    #[allow(non_snake_case)]
    fn getTransitionDomain(&self, datamodel: &mut dyn Datamodel, t: &Transition) -> StateId {
        #[cfg(feature = "Trace_Method")]
        self.tracer.enter_method("getTransitionDomain");
        #[cfg(feature = "Trace_Method")]
        self.tracer.trace_argument("t", &t);
        let tstates = self.getEffectiveTargetStates(datamodel, t);
        let domain;
        if tstates.isEmpty() {
            domain = 0;
        } else if t.transition_type == TransitionType::Internal
            && self.isCompoundState(t.source)
            && tstates.every(&|s| -> bool { self.isDescendant(*s, t.source) })
        {
            domain = t.source;
        } else {
            let mut l = List::new();
            l.push(t.source);
            domain = self.findLCCA(&l.append_set(&tstates));
        }
        #[cfg(feature = "Trace_Method")]
        self.tracer.trace_result("domain", &domain);
        #[cfg(feature = "Trace_Method")]
        self.tracer.exit_method("getTransitionDomain");
        domain
    }

    /// *W3C says*:
    /// # function findLCCA(stateList)
    /// The Least Common Compound Ancestor is the \<state\> or \<scxml\> element s such that s is a
    /// proper ancestor of all states on stateList and no descendant of s has this property.
    /// Note that there is guaranteed to be such an element since the <scxml> wrapper element is a
    /// common ancestor of all states. Note also that since we are speaking of proper ancestor
    /// (parent or parent of a parent, etc.) the LCCA is never a member of stateList.
    /// ```ignore
    /// function findLCCA(stateList):
    ///     for anc in getProperAncestors(stateList.head(),null).filter(isCompoundStateOrScxmlElement):
    ///         if stateList.tail().every(lambda s: isDescendant(s,anc)):
    ///             return anc;
    /// ```
    #[allow(non_snake_case)]
    fn findLCCA(&self, stateList: &List<StateId>) -> StateId {
        #[cfg(feature = "Trace_Method")]
        self.tracer.enter_method("findLCCA");
        #[cfg(feature = "Trace_Method")]
        self.tracer.trace_argument("stateList", stateList);
        let mut lcca = 0;
        for anc in self
            .getProperAncestors(*stateList.head(), 0)
            .toList()
            .filter_by(&|s| self.isCompoundStateOrScxmlElement(*s))
            .iterator()
        {
            if stateList.tail().every(&|s| self.isDescendant(*s, *anc)) {
                lcca = *anc;
                break;
            }
        }
        #[cfg(feature = "Trace_Method")]
        self.tracer.trace_result("lcca", &lcca);
        #[cfg(feature = "Trace_Method")]
        self.tracer.exit_method("findLCCA");
        lcca
    }

    /// *W3C says*:
    /// # function getEffectiveTargetStates(transition)
    /// Returns the states that will be the target when 'transition' is taken, dereferencing any history states.
    /// ```ignore
    /// function getEffectiveTargetStates(transition)
    ///     targets = new OrderedSet()
    ///     for s in transition.target
    ///         if isHistoryState(s):
    ///             if historyValue[s.id]:
    ///                 targets.union(historyValue[s.id])
    ///             else:
    ///                 targets.union(getEffectiveTargetStates(s.transition))
    ///         else:
    ///             targets.add(s)
    ///     return targets;
    /// ```
    #[allow(non_snake_case)]
    fn getEffectiveTargetStates(&self, datamodel: &mut dyn Datamodel, transition: &Transition) -> OrderedSet<StateId> {
        #[cfg(feature = "Trace_Method")]
        self.tracer.enter_method("getEffectiveTargetStates");
        #[cfg(feature = "Trace_Method")]
        self.tracer.trace_argument("transition", transition);
        let mut targets: OrderedSet<StateId> = OrderedSet::new();
        for sid in &transition.target {
            if self.isHistoryState(*sid) {
                if get_global!(datamodel).historyValue.has(*sid) {
                    targets.union(get_global!(datamodel).historyValue.get(*sid));
                } else {
                    let s = self.get_state_by_id(*sid);
                    // History states have exactly one "transition"
                    targets.union(
                        &self.getEffectiveTargetStates(datamodel, self.get_transition_by_id(*s.transitions.head())),
                    );
                }
            } else {
                targets.add(*sid);
            }
        }
        #[cfg(feature = "Trace_Method")]
        self.tracer.trace_result("targets", &targets);
        #[cfg(feature = "Trace_Method")]
        self.tracer.exit_method("getEffectiveTargetStates");
        targets
    }

    /// *W3C says*:
    /// # function getProperAncestors(state1, state2)
    /// If state2 is null, returns the set of all ancestors of state1 in ancestry order
    /// (state1's parent followed by the parent's parent, etc. up to an including the <scxml> element).
    /// If state2 is non-null, returns in ancestry order the set of all ancestors of state1,
    /// up to but not including state2.
    /// (A "proper ancestor" of a state is its parent, or the parent's parent,
    /// or the parent's parent's parent, etc.))
    /// If state2 is state1's parent, or equal to state1, or a descendant of state1, this returns the empty set.
    #[allow(non_snake_case)]
    fn getProperAncestors(&self, state1: StateId, state2: StateId) -> OrderedSet<StateId> {
        #[cfg(feature = "Trace_Method")]
        {
            self.tracer.enter_method("getProperAncestors");
            self.tracer.trace_argument("state1", &state1);
            self.tracer.trace_argument("state2", &state2);
        }

        let mut properAncestors: OrderedSet<StateId> = OrderedSet::new();
        if !self.isDescendant(state2, state1) {
            let mut currState = self.get_state_by_id(state1).parent;
            while currState != 0 && currState != state2 {
                properAncestors.add(currState);
                currState = self.get_state_by_id(currState).parent;
            }
        }
        #[cfg(feature = "Trace_Method")]
        {
            self.tracer
                .trace_result("properAncestors", &properAncestors);
            self.tracer.exit_method("getProperAncestors");
        }
        properAncestors
    }

    /// *W3C says*:
    /// function isDescendant(state1, state2)
    /// Returns 'true' if state1 is a descendant of state2 (a child, or a child of a child, or a child of a child of a child, etc.) Otherwise returns 'false'.
    #[allow(non_snake_case)]
    fn isDescendant(&self, state1: StateId, state2: StateId) -> bool {
        #[cfg(feature = "Trace_Method")]
        {
            self.tracer.enter_method("isDescendant");
            self.tracer.trace_argument("state1", &state1);
            self.tracer.trace_argument("state2", &state2);
        }
        let result;
        if state1 == 0 || state2 == 0 || state1 == state2 {
            result = false;
        } else {
            let mut currState = self.get_state_by_id(state1).parent;
            while currState != 0 && currState != state2 {
                currState = self.get_state_by_id(currState).parent;
            }
            result = currState == state2;
        }
        #[cfg(feature = "Trace_Method")]
        {
            self.tracer.trace_result("result", &result);
            self.tracer.exit_method("isDescendant");
        }
        result
    }

    /// *W3C says*:
    /// A Compound State: A state of type \<state\> with at least one child state.
    #[allow(non_snake_case)]
    fn isCompoundState(&self, state: StateId) -> bool {
        if state != 0 {
            let stateS = self.get_state_by_id(state);
            !(stateS.is_final || stateS.is_parallel || stateS.states.is_empty())
        } else {
            false
        }
    }

    #[allow(non_snake_case)]
    fn isCompoundStateOrScxmlElement(&self, sid: StateId) -> bool {
        if sid == self.pseudo_root {
            true
        } else {
            self.isCompoundState(sid)
        }
    }

    #[allow(non_snake_case)]
    fn isHistoryState(&self, state: StateId) -> bool {
        self.get_state_by_id(state).history_type != HistoryType::None
    }

    #[allow(non_snake_case)]
    fn isCancelEvent(&self, ev: &Event) -> bool {
        // Cancel-Events (outer fsm cancels a fsm instance that was started by some invoke)
        // are platform specific.
        ev.name.eq(EVENT_CANCEL_SESSION)
    }

    /// *W3C says*:
    /// function getChildStates(state1)
    /// Returns a list containing all \<state\>, \<final\>, and \<parallel\> children of state1.
    #[allow(non_snake_case)]
    fn getChildStates(&self, state1: StateId) -> List<StateId> {
        let mut l: List<StateId> = List::new();
        let stateRef = self.get_state_by_id(state1);
        for c in &stateRef.states {
            l.push(*c);
        }
        l
    }

    fn invoke(&mut self, datamodel: &mut dyn Datamodel, state_id: StateId, inv: &Invoke) {
        // W3C: if the evaluation of its arguments produces an error, the SCXML Processor must
        // terminate the processing of the element without further action.

        let type_name_data = match datamodel.get_expression_alternative_value(&inv.type_name, &inv.type_expr) {
            Ok(value) => value,
            Err(_) => {
                // Error -> abort
                return;
            }
        };

        let mut type_name = type_name_data.lock().unwrap().to_string();
        if type_name.eq(SCXML_INVOKE_TYPE_SHORT) {
            type_name = SCXML_INVOKE_TYPE.to_string();
        }

        if !(type_name.is_empty()
            || (type_name.starts_with(SCXML_INVOKE_TYPE) && type_name.len() <= (SCXML_INVOKE_TYPE.len() + 1)))
        {
            error!("Unsupported <invoke> type {}", type_name);
            return;
        }

        #[allow(non_snake_case)]
        let invokeId = if inv.invoke_id.is_empty() {
            // W3C:
            // A conformant SCXML document may specify either the 'id' or 'idlocation' attribute, but
            // must not specify both. If the 'idlocation' attribute is present, the SCXML Processor
            // must generate an id automatically when the <invoke> element is evaluated and store it
            // in the location specified by 'idlocation'. (In the rest of this document, we will refer
            // to this identifier as the "invokeid", regardless of whether it is specified by the
            // author or generated by the platform). The automatically generated identifier must have
            // the form stateid.platformid, where stateid is the id of the state containing this
            // element and platformid is automatically generated. platformid must be unique within
            // the current session.
            format!(
                "{}.{}",
                &inv.parent_state_name,
                PLATFORM_ID_COUNTER.fetch_add(1, Ordering::Relaxed)
            )
        } else {
            inv.invoke_id.clone()
        };

        let src = match datamodel.get_expression_alternative_value(&inv.src, &inv.src_expr) {
            Err(_) => {
                // Error -> Abort
                return;
            }
            Ok(value) => value.lock().unwrap().clone(),
        };
        let mut name_values: Vec<ParamPair> = Vec::new();
        for name in inv.name_list.as_slice() {
            match datamodel.get_by_location(name) {
                Err(_msg) => {
                    // Error -> Abort
                    return;
                }
                Ok(value) => {
                    name_values.push(ParamPair::new(name.as_str(), &value.lock().unwrap().clone()));
                }
            }
        }
        datamodel.evaluate_params(&inv.params, &mut name_values);

        #[cfg(feature = "Debug")]
        debug!(
            "Invoke: type '{}' invokeId '{}' src '{}' namelist '{:?}'",
            type_name, invokeId, src, name_values
        );

        // We currently don't check if id and idLocation are exclusive set.
        if !inv.external_id_location.is_empty() {
            // If "idlocation" is specified, we have to store the generated id to this location
            datamodel.set(
                inv.external_id_location.as_str(),
                Data::String(invokeId.clone()), true
            );
        }

        let result = if src.is_empty() {
            match datamodel.evaluate_content(&inv.content) {
                None => Err("No content to execute".to_string()),
                Some(content) => {
                    let mut global = get_global!(datamodel);
                    let session_id = global.session_id;

                    let actions = global.actions.get_copy();
                    global
                        .executor
                        .as_mut()
                        .unwrap()
                        .execute_with_data_from_xml(
                            content.lock().unwrap().to_string().as_str(),
                            actions,
                            &name_values,
                            Some(session_id),
                            &invokeId,
                            FinishMode::DISPOSE,
                            #[cfg(feature = "Trace")]
                            self.tracer.trace_mode(),
                        )
                }
            }
        } else {
            let mut global = get_global!(datamodel);
            let session_id = global.session_id;
            let actions = global.actions.get_copy();
            global.executor.as_mut().unwrap().execute_with_data(
                src.to_string().as_str(),
                actions,
                &name_values,
                Some(session_id),
                &invokeId,
                #[cfg(feature = "Trace")]
                self.tracer.trace_mode(),
            )
        };

        match result {
            Ok(mut session) => {
                session.state_id = Some(state_id);
                session.invoke_doc_id = inv.doc_id;

                get_global!(datamodel)
                    .child_sessions
                    .insert(invokeId, session);
            }
            Err(error) => {
                error!("Execute of '{}' failed: {}", src, error)
            }
        }
    }

    #[allow(non_snake_case)]
    fn cancelInvoke(&mut self, datamodel: &mut dyn Datamodel, invoke_id: &InvokeId, session_id: SessionId) {
        #[cfg(feature = "Trace_Method")]
        self.tracer.enter_method("cancelInvoke");
        get_global!(datamodel).child_sessions.remove(invoke_id);
        datamodel.send(
            SCXML_EVENT_PROCESSOR_SHORT_TYPE,
            &Data::String(format!("{}{}", SCXML_TARGET_SESSION_ID_PREFIX, session_id)),
            Event::new_simple(EVENT_CANCEL_SESSION),
        );
        #[cfg(feature = "Trace_Method")]
        self.tracer.exit_method("cancelInvoke");
    }

    /// *W3C says*:
    /// 5.9.1 Conditional Expressions
    /// Conditional expressions are used inside the 'cond' attribute of \<transition\>, \<if\> and \<elseif\>.
    /// If a conditional expression cannot be evaluated as a boolean value ('true' or 'false') or if
    /// its evaluation causes an error, the SCXML Processor must treat the expression as if it evaluated to
    /// 'false' and must place the error 'error.execution' in the internal event queue.
    ///
    /// See [Datamodel::execute_condition]
    #[allow(non_snake_case)]
    fn conditionMatch(&mut self, datamodel: &mut dyn Datamodel, tid: TransitionId) -> bool {
        let cond;
        {
            let t = self.get_transition_by_id_mut(tid);
            cond = t.cond.clone();
        }
        if cond.is_empty() {
            true
        } else {
            match datamodel.execute_condition(&cond) {
                Ok(v) => v,
                Err(_e) => {
                    datamodel.internal_error_execution();
                    false
                }
            }
        }
    }

    /// Converts a set of ids to list of references.
    fn set_to_state_list(&self, state_ids: &OrderedSet<StateId>) -> List<&State> {
        let mut l = List::new();
        for sid in state_ids.iterator() {
            l.push(self.get_state_by_id(*sid));
        }
        l
    }

    fn state_list_to_id_set(&self, states: &List<&State>) -> OrderedSet<StateId> {
        let mut l = OrderedSet::new();
        for state in states.iterator() {
            l.add(state.id);
        }
        l
    }

    /// Converts a set of Transition-ids to list of references.
    fn to_transition_list(&self, trans_ids: &List<TransitionId>) -> List<&Transition> {
        let mut l = List::new();
        for tid in trans_ids.iterator() {
            l.push(self.get_transition_by_id(*tid));
        }
        l
    }

    pub fn schedule<F>(&self, delay_ms: i64, mut cb: F) -> Option<Guard>
    where
        F: 'static + FnMut() + Send,
    {
        if delay_ms > 0 {
            Some(
                self.timer
                    .schedule_with_delay(chrono::Duration::milliseconds(delay_ms), cb),
            )
        } else {
            cb();
            None
        }
    }

    /// Very basic compare.
    /// TODO: needs to be extended if the tests get more complex.
    #[cfg(test)]
    pub fn compare_to(&self, other: &Fsm) -> bool {
        if self.name.eq(&other.name)
            && self.binding == other.binding
            && self.states.len() == other.states.len()
            && self.transitions.len() == other.transitions.len()
            && self.executableContent.len() == other.executableContent.len()
            && self.datamodel.eq(&other.datamodel)
        {
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct DoneData {
    /// content of \<content\> child
    pub content: Option<CommonContent>,

    /// \<param\> children
    pub params: Option<Vec<Parameter>>,
}

impl DoneData {
    pub fn new() -> DoneData {
        DoneData {
            content: None,
            params: None,
        }
    }
}

/// Stores all data for a State.
/// In this model "State" is used for SCXML elements "State" and "Parallel".
///
/// ## W3C says:
/// 3.3 \<state\>
/// Holds the representation of a state.
///
/// 3.3.1 Attribute Details
///
/// |Name| Required| Attribute Constraints|Type|Default Value|Valid Values|Description|
/// |----|---------|----------------------|----|-------------|------------|-----------|
/// |id|false|none|ID|none|A valid id as defined in [/doc/W3C_SCXML_2024_07_13/index.html#Schema](XML Schema)|The identifier for this state. See 3.14 IDs for details.|
/// |initial|false|MUST NOT be specified in conjunction with the \<initial\> element. MUST NOT occur in atomic states.|IDREFS|none|A legal state specification. See 3.11 Legal State Configurations and Specifications for details.|The id of the default initial state (or states) for this state.|
///
/// 3.3.2 Children
/// - \<onentry\> Optional element holding executable content to be run upon entering this \<state\>.
///    Occurs 0 or more times. See 3.8 \<onentry\>
/// - \<onexit\> Optional element holding executable content to be run when exiting this \<state\>.
///    Occurs 0 or more times. See 3.9 \<onexit\>
/// - \<transition\> Defines an outgoing transition from this state. Occurs 0 or more times.
///    See 3.5 \<transition\>
/// - \<initial\> In states that have substates, an optional child which identifies the default
///    initial state. Any transition which takes the parent state as its target will result in the
///    state machine also taking the transition contained inside the \<initial\> element.\
///    See 3.6 \<initial\>
/// - \<state\> Defines a sequential substate of the parent state. Occurs 0 or more times.
/// - \<parallel\> Defines a parallel substate. Occurs 0 or more times. See 3.4 \<parallel\>
/// - \<final\>. Defines a final substate. Occurs 0 or more times. See 3.7 \<final\>.
/// - \<history\> A child pseudo-state which records the descendant state(s) that the parent state
///    was in the last time the system transitioned from the parent.\
///    May occur 0 or more times. See 3.10 \<history\>.
/// - \<datamodel\> Defines part or all of the data model. Occurs 0 or 1 times. See 5.2 \<datamodel\>
/// - \<invoke> Invokes an external service. Occurs 0 or more times. See 6.4 \<invoke\> for details.
///
/// ##Definitions:
/// - An atomic state is a \<state\> that has no \<state\>, \<parallel\> or \<final\> children.
/// - A compound state is a \<state\> that has \<state\>, \<parallel\>, or \<final\> children
///    (or a combination of these).
/// - The default initial state(s) of a compound state are those specified by the 'initial' attribute
///    or \<initial\> element, if either is present. Otherwise it is the state's first child state
///    in document order.
///
/// In a conformant SCXML document, a compound state may specify either an "initial" attribute or an
/// \<initial\> element, but not both. See 3.6 \<initial\> for a discussion of the difference between
/// the two notations.
#[allow(non_snake_case)]
pub struct State {
    /// The internal Id (not W3C). Used to refence the state.
    /// Index+1 of the state in Fsm.states
    pub id: StateId,

    /// The unique id, counting in document order.
    /// "id" is increasing on references to states, not declaration and may not result in correct order.
    pub doc_id: DocumentId,

    /// The SCXML id.
    pub name: String,

    /// The initial transition id (if the state has sub-states).
    pub initial: TransitionId,

    /// The Ids of the sub-states of this state.
    pub states: Vec<StateId>,

    /// True for "parallel" states
    pub is_parallel: bool,

    /// True for "final" states
    pub is_final: bool,

    pub history_type: HistoryType,

    /// The script that is executed if the state is entered. See W3c comments for \<onentry\> above.
    pub onentry: Vec<ExecutableContentId>,

    /// The script that is executed if the state is left. See W3c comments for \<onexit\> above.
    pub onexit: Vec<ExecutableContentId>,

    /// All transitions between sub-states.
    pub transitions: List<TransitionId>,

    pub invoke: List<Invoke>,
    pub history: List<StateId>,

    /// The initial data values on this state.
    pub data: HashMap<String, DataArc>,

    /// True if the state was never entered before.
    pub isFirstEntry: bool,

    pub parent: StateId,
    pub donedata: Option<DoneData>,
}

impl State {
    pub fn new(name: &str) -> State {
        State {
            id: 0,
            doc_id: 0,
            name: name.to_string(),
            initial: 0,
            states: vec![],
            onentry: vec![],
            onexit: vec![],
            transitions: List::new(),
            is_parallel: false,
            is_final: false,
            history_type: HistoryType::None,
            data: HashMap::new(),
            isFirstEntry: true,
            parent: 0,
            donedata: None,
            invoke: List::new(),
            history: List::new(),
        }
    }
}

impl Clone for State {
    fn clone(&self) -> Self {
        todo!()
    }
}

impl PartialEq for State {
    fn eq(&self, _other: &Self) -> bool {
        todo!()
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
#[repr(u8)]
pub enum HistoryType {
    Shallow,
    Deep,
    None,
}

impl HistoryType {
    pub fn ordinal(&self) -> u8 {
        match self {
            HistoryType::Shallow => 1u8,
            HistoryType::Deep => 2u8,
            HistoryType::None => 0u8,
        }
    }

    pub fn from_ordinal(v: u8) -> HistoryType {
        match v {
            1 => HistoryType::Shallow,
            2 => HistoryType::Deep,
            _ => HistoryType::None,
        }
    }
}

pub fn map_history_type(ts: &String) -> HistoryType {
    match ts.to_lowercase().as_str() {
        "deep" => HistoryType::Deep,
        "shallow" => HistoryType::Shallow,
        "" => HistoryType::None,
        _ => panic!("Unknown transition type '{}'", ts),
    }
}

#[derive(Debug, PartialEq, Default)]
#[repr(u8)]
pub enum TransitionType {
    Internal,
    #[default]
    External,
}

impl TransitionType {
    pub fn from_ordinal(ordinal: u8) -> TransitionType {
        match ordinal {
            0 => TransitionType::Internal,
            1 => TransitionType::External,
            _ => {
                panic!("Unknown ordinal {} for TransitionType", ordinal);
            }
        }
    }

    pub fn ordinal(&self) -> u8 {
        match self {
            TransitionType::Internal => 0u8,
            TransitionType::External => 1u8,
        }
    }
}

pub fn map_transition_type(ts: &String) -> TransitionType {
    match ts.to_lowercase().as_str() {
        "internal" => TransitionType::Internal,
        "external" => TransitionType::External,
        "" => TransitionType::External,
        _ => panic!("Unknown transition type '{}'", ts),
    }
}

pub(crate) static ID_COUNTER: AtomicU32 = AtomicU32::new(1);
pub(crate) static SESSION_ID_COUNTER: AtomicU32 = AtomicU32::new(1);

pub type TransitionId = u32;

/// A state to state transition with references to content that shall be executed with the transition.
#[derive(Debug, Default)]
pub struct Transition {
    pub id: TransitionId,
    pub doc_id: DocumentId,

    // TODO: Possibly we need some type to express event ids
    pub events: Vec<String>,
    pub wildcard: bool,
    pub cond: Data,
    pub source: StateId,
    pub target: Vec<StateId>,
    pub transition_type: TransitionType,
    pub content: ExecutableContentId,
}

impl PartialEq for Transition {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Transition {
    pub fn new() -> Transition {
        let idc = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        Transition {
            id: idc,
            doc_id: 0,
            events: vec![],
            wildcard: false,
            cond: Data::Null(),
            source: 0,
            target: vec![],
            transition_type: TransitionType::External,
            content: 0,
        }
    }

    #[allow(non_snake_case)]
    /// W3C says:\
    /// An event descriptor matches an event name if its string of tokens is an exact match or a prefix
    /// of the set of tokens in the event's name. In all cases, the token matching is case sensitive.\
    /// For example, a transition with an 'event' attribute of "error foo" will match event names
    /// "error", "error.send", "error.send.failed", etc. (or "foo", "foo.bar" etc.) but would not
    /// match events named "errors.my.custom", "errorhandler.mistake", "error.send" or "foobar".\
    /// For compatibility with CCXML, and to make the prefix matching possibly more clear to a reader
    /// of the SCXML document, an event descriptor MAY also end with the wildcard '.*', which matches
    /// zero or more tokens at the end of the processed event's name. Note that a transition with
    /// 'event' of "error", one with "error.", and one with "error.*" are functionally equivalent
    /// since they are token prefixes of exactly the same set of event names.
    ///
    /// Implementation Note:
    /// Terminating "." and ".*" are already stripped by the parser.
    fn nameMatch(&self, name: &str) -> bool {
        if self.wildcard {
            true
        } else {
            for e in &self.events {
                if name.starts_with(e) {
                    if name.len() == e.len() {
                        // Full match
                        return true;
                    } else if let Some(c) = name.chars().nth(e.len()) {
                        // partial match, token needs to be terminated with "."
                        if c == '.' {
                            return true;
                        }
                    }
                }
            }
            false
        }
    }
}

lazy_static! {
    static ref datamodel_factories: Arc<Mutex<HashMap<String, Box<dyn DatamodelFactory>>>> = {
        let mut hs: HashMap<String, Box<dyn DatamodelFactory>> = HashMap::new();

        #[cfg(feature = "ECMAScript")]
        hs.insert(
            ECMA_SCRIPT_LC.to_string(),
            Box::new(ECMAScriptDatamodelFactory {}),
        );
        hs.insert(
            NULL_DATAMODEL_LC.to_string(),
            Box::new(NullDatamodelFactory {}),
        );
        #[cfg(feature = "RfsmExpressionModel")]
        hs.insert(
            RFSM_EXPRESSION_DATAMODEL_LC.to_string(),
            Box::new(RFsmExpressionDatamodelFactory {}),
        );

        Arc::new(Mutex::new(hs))
    };
}

/// Register a new Datamodel.\
/// The name is case-insensitive.
pub fn register_datamodel(name: &str, factory: Box<dyn DatamodelFactory>) {
    datamodel_factories
        .lock()
        .unwrap()
        .insert(name.to_lowercase(), factory);
}

pub fn create_datamodel(
    name: &str,
    global_data: GlobalDataArc,
    options: &HashMap<String, String>,
) -> Box<dyn Datamodel> {
    match datamodel_factories
        .lock()
        .unwrap()
        .get_mut(&name.to_lowercase())
    {
        Some(factory) => factory.create(global_data, options),
        None => panic!("Unsupported Data Model '{}'", name),
    }
}

////////////////////////////////////////
/// Display support

impl Display for Fsm {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Fsm{{v:{} root:{} states:",
            self.version, self.pseudo_root
        )?;
        display_state_map(&self.states, f)?;
        display_transition_map(&self.transitions, f)?;
        write!(f, "}}")
    }
}

impl Display for State {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{{#{} states:{} transitions: {}}}",
            self.id,
            vec_to_string(&self.states),
            vec_to_string(&self.transitions.data)
        )
    }
}

impl Display for Transition {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{{#{} {} {:?} target:{:?}}}",
            self.id, self.transition_type, &self.events, self.target
        )
    }
}

impl Display for TransitionType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TransitionType::Internal => f.write_str("internal"),
            TransitionType::External => f.write_str("external"),
        }
    }
}

impl Display for List<u32> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(vec_to_string(&self.data).as_str())
    }
}

impl Display for List<&State> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(vec_to_string(&self.data).as_str())
    }
}

impl Display for OrderedSet<u32> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(vec_to_string(&self.data).as_str())
    }
}

pub(crate) fn vec_to_string<T: Display>(v: &[T]) -> String {
    let mut s = "[".to_string();

    for (i, vi) in v.iter().enumerate() {
        s += format!("{}{}", if i > 0 { "," } else { "" }, vi).as_str();
    }
    s += "]";
    s
}

pub(crate) fn opt_vec_to_string<T: Display>(v: &Option<Vec<T>>) -> String {
    match v {
        None => "None".to_string(),
        Some(v) => {
            let mut s = "[".to_string();

            for (i, vi) in v.iter().enumerate() {
                s += format!("{}{}", if i > 0 { "," } else { "" }, vi).as_str();
            }
            s += "]";
            s
        }
    }
}

#[derive(Clone)]
pub struct DebugAction {}

impl DebugAction {
    pub fn new() -> DebugAction {
        DebugAction {}
    }
}

impl Default for DebugAction {
    fn default() -> Self {
        Self::new()
    }
}

impl Action for DebugAction {
    fn execute(&self, arguments: &[Data], _global: &GlobalData) -> Result<Data, String> {
        let mut i = 0;
        for data in arguments {
            i += 1;
            println!(" {}: {}", i, data)
        }
        Ok(Data::Boolean(true))
    }

    fn get_copy(&self) -> Box<dyn Action> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::fsm::OrderedSet;
    use crate::fsm::{EventType, List};
    #[cfg(feature = "Trace")]
    use crate::tracer::TraceMode;
    use std::collections::HashMap;

    #[cfg(feature = "ECMAScript")]
    #[cfg(feature = "xml")]
    use crate::scxml_reader;

    #[cfg(feature = "ECMAScript")]
    #[cfg(feature = "xml")]
    use std::sync::mpsc::Sender;

    use crate::test::run_test_manual_with_send;
    #[cfg(feature = "ECMAScript")]
    #[cfg(feature = "xml")]
    use crate::Event;

    #[cfg(feature = "ECMAScript")]
    #[cfg(feature = "xml")]
    fn test_send(sender: &Sender<Box<Event>>, e: Event) {
        let _r = sender.send(Box::new(e));
    }

    #[test]
    fn list_can_can_push() {
        let mut l: List<String> = List::new();

        l.push("Abc".to_string());
        l.push("def".to_string());
        l.push("ghi".to_string());
        l.push("xyz".to_string());
        assert_eq!(l.size(), 4);
    }

    #[test]
    fn list_can_head() {
        let mut l1: List<String> = List::new();

        l1.push("Abc".to_string());
        l1.push("def1".to_string());
        l1.push("ghi1".to_string());

        assert_eq!(l1.head(), &"Abc".to_string());
    }

    #[test]
    fn list_can_tail() {
        let mut l1: List<String> = List::new();

        l1.push("Abc".to_string());
        l1.push("def1".to_string());
        l1.push("ghi1".to_string());

        assert_eq!(l1.tail().size(), 2);
        assert_eq!(l1.size(), 3);
    }

    #[test]
    fn list_can_append() {
        let mut l1: List<String> = List::new();

        l1.push("Abc".to_string());
        l1.push("def1".to_string());
        l1.push("ghi1".to_string());
        l1.push("xyz1".to_string());

        let mut l2: List<String> = List::new();
        l2.push("Abc".to_string());
        l2.push("def2".to_string());
        l2.push("ghi2".to_string());
        l2.push("xyz2".to_string());

        let l3 = l1.append(&l2);
        assert_eq!(l3.size(), l1.size() + l2.size());

        let l4 = l1.append(&l1);
        assert_eq!(l4.size(), 2 * l1.size());
    }

    #[test]
    fn list_can_some() {
        let mut l: List<String> = List::new();
        l.push("Abc".to_string());
        l.push("def".to_string());
        l.push("ghi".to_string());
        l.push("xyz".to_string());

        let m = l.some(&|s| -> bool { *s == "Abc" });

        assert!(m);
    }

    #[test]
    fn list_can_every() {
        let mut l: List<String> = List::new();
        l.push("Abc".to_string());
        l.push("def".to_string());
        l.push("ghi".to_string());
        l.push("xyz".to_string());

        let mut m = l.every(&|_s| -> bool { true });
        assert!(m);

        m = l.every(&|s| -> bool { !s.eq(&"ghi".to_string()) });
        assert!(!m);
    }

    #[test]
    fn list_can_filter() {
        let mut l: List<String> = List::new();
        l.push("Abc".to_string());
        l.push("def".to_string());
        l.push("ghi".to_string());
        l.push("xyz".to_string());

        let l2: List<String> = l.filter_by(&|_s: &String| -> bool { true });
        assert_eq!(l2.size(), l.size());

        let l3 = l2.filter_by(&|_s: &String| -> bool { false });
        assert_eq!(l3.size(), 0);
    }

    #[test]
    fn list_can_sort() {
        let mut l1: List<String> = List::new();
        l1.push("Xyz".to_string());
        l1.push("Bef".to_string());
        l1.push("Ghi".to_string());
        l1.push("Abc".to_string());

        println!("Unsorted ====");
        let mut l1v: Vec<String> = Vec::new();

        let mut l2 = l1.sort(&|a, b| a.partial_cmp(b).unwrap());

        while l1.size() > 0 {
            let e = l1.head();
            println!(" {}", e);
            l1v.push(e.clone());
            l1 = l1.tail();
        }
        l1v.sort_by(&|a: &String, b: &String| a.partial_cmp(b).unwrap());

        assert_eq!(l1v.len(), l2.size());

        println!("Sorted ======");
        let mut i = 0;
        while l2.size() > 0 {
            let h = l2.head().clone();
            l2 = l2.tail();
            println!(" {}", h);
            assert!(h.eq(l1v.get(i).unwrap()));
            i += 1;
        }
        println!("=============");
    }

    #[test]
    fn ordered_set_can_add_and_delete() {
        let mut os: OrderedSet<String> = OrderedSet::new();

        os.add("Abc".to_string());
        os.add("def".to_string());
        os.add("ghi".to_string());
        os.add("xyz".to_string());
        assert_eq!(os.size(), 4);

        os.delete(&"Abc".to_string());
        os.delete(&"ghi".to_string());
        os.delete(&"xxx".to_string());
        os.delete(&"Abc".to_string()); // should be ignored.

        assert_eq!(os.size(), 2);
    }

    #[test]
    fn ordered_set_can_union() {
        let mut os1: OrderedSet<String> = OrderedSet::new();

        os1.add("Abc".to_string());
        os1.add("def1".to_string());
        os1.add("ghi1".to_string());
        os1.add("xyz1".to_string());

        let mut os2: OrderedSet<String> = OrderedSet::new();
        os2.add("Abc".to_string());
        os2.add("def2".to_string());
        os2.add("ghi2".to_string());
        os2.add("xyz2".to_string());

        os1.union(&os2);

        assert_eq!(os1.size(), 7);
        assert!(os1.isMember(&"def2".to_string()));
        assert!(os1.isMember(&"Abc".to_string()));
    }

    #[test]
    #[allow(non_snake_case)]
    fn ordered_set_can_toList() {
        let mut os: OrderedSet<String> = OrderedSet::new();
        os.add("Abc".to_string());
        os.add("def".to_string());
        os.add("ghi".to_string());
        os.add("xyz".to_string());

        let l = os.toList();

        assert_eq!(l.size(), os.size());
    }

    #[test]
    fn ordered_set_can_some() {
        let mut os: OrderedSet<String> = OrderedSet::new();
        os.add("Abc".to_string());
        os.add("def".to_string());
        os.add("ghi".to_string());
        os.add("xyz".to_string());

        let m = os.some(&|s| -> bool { *s == "Abc" });

        assert!(m);
    }

    #[test]
    fn ordered_set_can_every() {
        let mut os: OrderedSet<String> = OrderedSet::new();
        os.add("Abc".to_string());
        os.add("def".to_string());
        os.add("ghi".to_string());
        os.add("xyz".to_string());

        let mut m = os.every(&|_s| -> bool { true });
        assert!(m);

        m = os.every(&|s| -> bool { !s.eq(&"ghi".to_string()) });
        assert!(!m);
    }

    #[test]
    #[allow(non_snake_case)]
    fn ordered_set_can_hasIntersection() {
        let mut os1: OrderedSet<String> = OrderedSet::new();
        os1.add("Abc".to_string());
        os1.add("def".to_string());
        os1.add("ghi".to_string());
        os1.add("xyz".to_string());

        let mut os2: OrderedSet<String> = OrderedSet::new();

        let mut m = os1.hasIntersection(&os2);
        assert!(!m);

        // One common elements
        os2.add("Abc".to_string());
        m = os1.hasIntersection(&os2);
        assert!(m);

        // Same other un-common elements
        os2.add("Def".to_string());
        os2.add("Ghi".to_string());
        os2.add("Xyz".to_string());
        m = os1.hasIntersection(&os2);
        assert!(m);

        // Same with TWO common elements
        os2.add("def".to_string());
        m = os1.hasIntersection(&os2);
        assert!(m);

        // Remove common elements from first
        os1.delete(&"Abc".to_string());
        os1.delete(&"def".to_string());
        m = os1.hasIntersection(&os2);
        assert!(!m);

        // Always common with itself
        m = os1.hasIntersection(&os1);
        assert!(m);

        // but not if empty
        os1.clear();
        m = os1.hasIntersection(&os1);
        // Shall return false
        assert!(!m);
    }

    #[test]
    #[allow(non_snake_case)]
    fn ordered_set_can_isEmpty() {
        let mut os1: OrderedSet<String> = OrderedSet::new();
        assert!(os1.isEmpty());

        os1.add("Abc".to_string());
        assert!(!os1.isEmpty());
    }

    #[test]
    fn ordered_set_can_clear() {
        let mut os1: OrderedSet<String> = OrderedSet::new();
        os1.add("Abc".to_string());
        os1.clear();
        assert!(os1.isEmpty());
    }

    #[test]
    #[cfg(feature = "ECMAScript")]
    #[cfg(feature = "xml")]
    fn fsm_shall_exit() {
        // init_logging();
        println!("Creating The SM:");
        let sm = scxml_reader::parse_from_xml(
            r"<scxml initial='Main' datamodel='ecmascript'>
      <script>
        log('Hello World', ' again ');
        log('Hello Again');
      </script>
      <state id='Main'>
        <initial>
          <transition target='MainA'/>
        </initial>
        <state id='MainA'>
          <transition event='a ab abc' cond='true' type='internal' target='finalMe'/>
        </state>
        <state id='MainB'>
        </state>
        <final id='finalMe'>
          <onentry>
            <log label='info' expr='Date.now()'/>
          </onentry>
        </final>
        <transition event='exit' cond='true' type='internal' target='OuterFinal'/>
      </state>
      <final id='OuterFinal'>
      </final>
    </scxml>"
                .to_string(),
        );

        assert!(sm.is_ok(), "FSM shall be parsed");

        let fsm = sm.unwrap();

        let expected_config = vec!["OuterFinal".to_string()];

        assert!(
            run_test_manual_with_send(
                "fsm_shall_exit",
                &HashMap::new(),
                fsm,
                &Vec::new(),
                #[cfg(feature = "Trace")]
                TraceMode::ALL,
                2000,
                &expected_config,
                |sender| {
                    println!("Send Event");
                    test_send(
                        &sender,
                        Event {
                            name: "ab".to_string(),
                            etype: EventType::platform,
                            sendid: Some("0".to_string()),
                            origin: None,
                            origin_type: None,
                            invoke_id: None,
                            param_values: None,
                            content: None,
                        },
                    );
                    test_send(
                        &sender,
                        Event {
                            name: "exit".to_string(),
                            etype: EventType::platform,
                            sendid: Some("0".to_string()),
                            origin: None,
                            origin_type: None,
                            invoke_id: None,
                            param_values: None,
                            content: None,
                        },
                    );
                },
            ),
            "FSM shall terminate with state 'OuterFinal'"
        );
    }
}
