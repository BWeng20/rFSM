//! Implements the W3C recommendation algorithm.
//! See [W3C:Algorithm for SCXML Interpretation](https://www.w3.org/TR/scxml/#AlgorithmforSCXMLInterpretation)

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use std::{fmt, thread};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::{Debug, Display, Formatter};
use std::hash::Hash;
use std::ops::DerefMut;
use std::slice::Iter;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::JoinHandle;

use log::info;

#[cfg(feature = "ECMAScript")]
use crate::ecma_script_datamodel::{ECMA_SCRIPT_LC, ECMAScriptDatamodel};
use crate::executable_content::ExecutableContent;

pub const NULL_DATAMODEL: &str = "NULL";
pub const NULL_DATAMODEL_LC: &str = "null";

/// Starts the FSM inside a worker thread.
///
pub fn start_fsm(mut sm: Box<Fsm>) -> (JoinHandle<()>, Sender<Box<Event>>) {
    let externalQueue: BlockingQueue<Box<Event>> = BlockingQueue::new();
    let sender = externalQueue.sender.clone();
    let thread = thread::Builder::new().name("fsm_interpret".to_string()).spawn(
        move || {
            info!("SM starting...");
            {
                let mut datamodel = crate::fsm::createDatamodel(sm.datamodel.as_str());
                datamodel.global().externalQueue = externalQueue;
                sm.interpret(datamodel.deref_mut());
            }
            info!("SM finished");
        });

    (thread.unwrap(), sender)
}


////////////////////////////////////////////////////////////////////////////////
// ## Implementation of the data-structures and algorithms described in the W3C scxml proposal.
// As reference each type and method has the w3c description as documentation.
// See https://www.w3.org/TR/scxml/#AlgorithmforSCXMLInterpretation

////////////////////////////////////////////////////////////////////////////////
// ## General Purpose Data types
// Structs and methods are designed to match the signatures in the W3c-Pseudo-code.


/// ## General Purpose List type
#[derive(Clone)]
pub struct List<T: Clone> {
    data: Vec<T>,
}

impl<T: Clone + PartialEq> List<T> {
    pub fn new() -> List<T> {
        List { data: Default::default() }
    }

    pub fn from_array(l: &[T]) -> List<T> {
        List { data: l.to_vec() }
    }

    pub fn size(&self) -> usize {
        self.data.len()
    }

    pub fn push(&mut self, t: T) {
        self.data.push(t);
    }

    /// #W3C says:
    /// Returns the head of the list
    pub fn head(&self) -> &T {
        self.data.first().unwrap()
    }

    /// #W3C says:
    /// Returns the tail of the list (i.e., the rest of the list once the head is removed)
    pub fn tail(&self) -> List<T> {
        let mut t = List {
            data: self.data.clone()
        };
        t.data.remove(0);
        t
    }

    /// #W3C says:
    /// Returns the list appended with l
    pub fn append(&self, l: &List<T>) -> List<T> {
        let mut t = List {
            data: self.data.clone()
        };
        for i in l.data.iter()
        {
            t.data.push((*i).clone());
        }
        t
    }

    /// #W3C says:
    /// Returns the list appended with l
    pub fn appendSet(&self, l: &OrderedSet<T>) -> List<T> {
        let mut t = List {
            data: self.data.clone()
        };
        for i in l.data.iter()
        {
            t.data.push((*i).clone());
        }
        t
    }

    /// #W3C says:
    /// Returns the list of elements that satisfy the predicate f
    /// # Actual Implementation:
    /// Can't name the function "filter" because this get in conflict with pre-defined "filter"
    /// that is introduced by the Iterator-implementation.
    pub fn filterBy(&self, f: &dyn Fn(&T) -> bool) -> List<T> {
        let mut t = List::new();

        for i in self.data.iter() {
            if f(&(*i)) {
                t.data.push((*i).clone());
            }
        }
        t
    }

    /// #W3C says:
    /// Returns true if some element in the list satisfies the predicate f.  Returns false for an empty list.
    pub fn some(&self, f: &dyn Fn(&T) -> bool) -> bool {
        for si in &self.data {
            if f(si) {
                return true;
            }
        }
        false
    }

    /// #W3C says:
    /// Returns true if every element in the list satisfies the predicate f.  Returns true for an empty list.
    pub fn every(&self, f: &dyn Fn(&T) -> bool) -> bool {
        for si in &self.data {
            if !f(si) {
                return false;
            }
        }
        true
    }

    pub fn sort<F>(&self, compare: &F) -> List<T>
        where
            F: Fn(&T, &T) -> std::cmp::Ordering + ?Sized {
        let mut t = List {
            data: self.data.clone()
        };
        t.data.sort_by(compare);
        t
    }

    pub fn iterator(&self) -> Iter<'_, T> {
        self.data.iter()
    }

    pub fn toSet(&self) -> OrderedSet<T> {
        let mut s = OrderedSet::new();
        for e in self.data.iter()
        {
            s.add(e.clone());
        }
        s
    }

    pub fn last_mut(&mut self) -> &mut T {
        self.data.last_mut().unwrap()
    }
}

/// Set datatype used by the algorithm,
/// #W3C says:
/// Note that the algorithm assumes a Lisp-like semantics in which the empty Set null is equivalent
/// to boolean 'false' and all other entities are equivalent to 'true'.
///
/// The notation [...] is used as a list constructor, so that '[t]' denotes a list whose only member
/// is the object t.
#[derive(Debug)]
#[derive(Clone)]
pub struct OrderedSet<T> {
    data: Vec<T>,
}

impl<T: Clone + PartialEq> OrderedSet<T> {
    pub fn new() -> OrderedSet<T> {
        OrderedSet { data: Default::default() }
    }

    pub fn from_array(l: &[T]) -> OrderedSet<T> {
        OrderedSet { data: l.to_vec() }
    }

    /// Extension: The size (only informational)
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// #W3C says:
    /// Adds e to the set if it is not already a member
    pub fn add(&mut self, e: T) {
        if !self.data.contains(&e) {
            self.data.push(e.clone());
        }
    }

    /// #W3C says:
    /// Deletes e from the set
    pub fn delete(&mut self, e: &T) {
        self.data.retain(|x| *x != *e);
    }

    /// #W3C says:
    /// Adds all members of s that are not already members of the set
    /// (s must also be an OrderedSet)
    pub fn union(&mut self, s: &OrderedSet<T>) {
        for si in &s.data {
            if !self.isMember(&si) {
                self.add(si.clone());
            }
        }
    }

    /// #W3C says:
    /// Is e a member of set?
    pub fn isMember(&self, e: &T) -> bool {
        self.data.contains(e)
    }

    /// #W3C says:
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

    /// #W3C says:
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

    /// #W3C says:
    /// Returns true if this set and set s have at least one member in common
    pub fn hasIntersection(&self, s: &OrderedSet<T>) -> bool {
        for si in &self.data {
            if s.isMember(si) {
                return true;
            }
        }
        false
    }

    /// #W3C says:
    /// Is the set empty?
    pub fn isEmpty(&self) -> bool {
        self.size() == 0
    }

    /// #W3C says:
    /// Remove all elements from the set (make it empty)
    pub fn clear(&mut self) {
        self.data.clear();
    }

    /// #W3C says:
    /// Converts the set to a list that reflects the order in which elements were originally added.
    ///
    /// In the case of sets created by intersection, the order of the first set (the one on which
    /// the method was called) is used
    ///
    /// In the case of sets created by union, the members of the first set (the one on which union
    /// was called) retain their original ordering while any members belonging to the second set only
    /// are placed after, retaining their ordering in their original set.
    pub fn toList(&self) -> List<T> {
        let mut l = List::new();
        for e in self.data.iter()
        {
            l.push(e.clone());
        }
        l
    }

    pub fn sort<F>(&self, compare: &F) -> List<T>
        where
            F: Fn(&T, &T) -> std::cmp::Ordering + ?Sized {
        let mut t = List {
            data: self.data.clone()
        };
        t.data.sort_by(compare);
        t
    }


    pub fn iterator(&self) -> Iter<'_, T> {
        self.data.iter()
    }
}

#[derive(Debug)]
pub struct Queue<T> {
    data: VecDeque<T>,
}

impl<T> Queue<T> {
    fn new() -> Queue<T> {
        Queue {
            data: VecDeque::new()
        }
    }

    /// Extension to re-use exiting instances.
    pub fn clear(&mut self) {
        self.data.clear();
    }

    /// #W3C says:
    /// Puts e last in the queue
    pub fn enqueue(&mut self, e: T) {
        self.data.push_back(e);
    }

    /// #W3C says:
    /// Removes and returns first element in queue
    pub fn dequeue(&mut self) -> T {
        self.data.pop_front().unwrap()
    }

    /// #W3C says:
    /// Is the queue empty?
    pub fn isEmpty(&self) -> bool {
        self.data.is_empty()
    }
}

#[derive(Debug)]
pub struct BlockingQueue<T> {
    pub sender: Sender<T>,
    pub receiver: Arc<Mutex<Receiver<T>>>,
}

impl<T> BlockingQueue<T> {
    fn new() -> BlockingQueue<T> {
        let (sender, receiver) = channel();
        BlockingQueue {
            receiver: Arc::new(Mutex::new(receiver)),
            sender,
        }
    }

    /// #W3C says:
    /// Puts e last in the queue
    pub fn enqueue(&mut self, e: T) {
        self.sender.send(e).unwrap()
    }

    /// #W3C says:
    /// Removes and returns first element in queue, blocks if queue is empty
    pub fn dequeue(&mut self) -> T {
        self.receiver.lock().unwrap().recv().unwrap()
    }
}

/// #W3C says:
/// table[foo] returns the value associated with foo.
/// table[foo] = bar sets the value associated with foo to be bar.
/// #Actual implementation:
/// Instead of the Operators, methods are used.
#[derive(Debug)]
pub struct HashTable<K, T> {
    data: HashMap<K, T>,
}

impl<K: std::cmp::Eq + Hash + Clone, T: Clone> HashTable<K, T> {
    fn new() -> HashTable<K, T> {
        HashTable { data: HashMap::new() }
    }
    /// Extension to re-use exiting instances.
    pub fn clear(&mut self) {
        self.data.clear();
    }

    pub fn put(&mut self, k: K, v: &T) {
        self.data.insert(k.clone(), v.clone());
    }

    pub fn putMove(&mut self, k: K, v: T) {
        self.data.insert(k.clone(), v);
    }

    pub fn putAll(&mut self, t: &HashTable<K, T>) {
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

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum BindingType {
    Early,
    Late,
}

impl FromStr for BindingType {
    type Err = ();

    fn from_str(input: &str) -> Result<BindingType, Self::Err> {
        match input.to_lowercase().as_str() {
            "early" => Ok(BindingType::Early),
            "Late" => Ok(BindingType::Late),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum EventType {
    /// for events raised by the platform itself, such as error events
    platform,
    /// for events raised by \<raise\> and \<send\> with target '_internal'
    internal,
    /// for all other events
    external,

}

/// #W3c says:
/// ##The Internal Structure of Events
/// Events have an internal structure which is reflected in the _event variable. This variable can be accessed to condition transitions (via boolean expressions in the 'cond' attribute) or to update the data model (via <assign>), etc.
///
/// The SCXML Processor must ensure that the following fields are present in all events, whether internal or external.
///
/// - name. This is a character string giving the name of the event. The SCXML Processor must set the name field to the name of this event. It is what is matched against the 'event' attribute of \<transition\>. Note that transitions can do additional tests by using the value of this field inside boolean expressions in the 'cond' attribute.
/// - type. This field describes the event type. The SCXML Processor must set it to: "platform" (for events raised by the platform itself, such as error events), "internal" (for events raised by \<raise\> and \<send\> with target '_internal') or "external" (for all other events).
/// - sendid. If the sending entity has specified a value for this, the Processor must set this field to that value (see C Event I/O Processors for details). Otherwise, in the case of error events triggered by a failed attempt to send an event, the Processor must set this field to the send id of the triggering <send> element. Otherwise it must leave it blank.
/// - origin. This is a URI, equivalent to the 'target' attribute on the \<send\> element. For external events, the SCXML Processor should set this field to a value which, when used as the value of 'target', will allow the receiver of the event to <send> a response back to the originating entity via the Event I/O Processor specified in 'origintype'. For internal and platform events, the Processor must leave this field blank.
/// - origintype. This is equivalent to the 'type' field on the <send> element. For external events, the SCXML Processor should set this field to a value which, when used as the value of 'type', will allow the receiver of the event to <send> a response back to the originating entity at the URI specified by 'origin'. For internal and platform events, the Processor must leave this field blank.
/// - invokeid. If this event is generated from an invoked child process, the SCXML Processor must set this field to the invoke id of the invocation that triggered the child process. Otherwise it must leave it blank.
/// - data. This field contains whatever data the sending entity chose to include in this event. The receiving SCXML Processor should reformat this data to match its data model, but must not otherwise modify it. If the conversion is not possible, the Processor must leave the field blank and must place an error 'error.execution' in the internal event queue.
///
#[derive(Debug)]
pub struct Event {
    pub name: String,
    pub etype: EventType,
    pub sendid: u32,
    pub origin: String,
    pub origintype: String,
    pub invokeid: InvokeId,
    pub data: Option<DoneData>,
}

impl ToString for Event {
    fn to_string(&self) -> String {
        self.name.to_string()
    }
}

impl Data for Event {
    fn get_copy(&self) -> Box<dyn Data> {
        Event::get_copy(self)
    }
}

impl Event {
    pub fn new(prefix: &str, id: &String, ev_data: &Option<DoneData>) -> Event {
        Event {
            name: format!("{}{}", prefix, id),
            etype: EventType::external,
            sendid: 0,
            origin: "".to_string(),
            data: ev_data.clone(),
            invokeid: 0,
            origintype: "".to_string(),
        }
    }

    pub fn trace(t: Trace, enable: bool) -> Event {
        Event {
            name: format!("trace.{}.{}", t, if enable { "on" } else { "off" }),
            etype: EventType::external,
            sendid: 0,
            origin: "".to_string(),
            data: None,
            invokeid: 0,
            origintype: "".to_string(),
        }
    }

    pub fn error(name: &str) -> Event {
        Event {
            name: format!("error.{}", name),
            etype: EventType::platform,
            sendid: 0,
            origin: "".to_string(),
            data: None,
            invokeid: 0,
            origintype: "".to_string(),
        }
    }

    pub fn get_copy(&self) -> Box<Event> {
        Box::new(Event {
            invokeid: self.invokeid,
            data: self.data.clone(),
            name: self.name.clone(),
            etype: self.etype,
            sendid: self.sendid,
            origin: self.origin.clone(),
            origintype: self.origintype.clone(),
        })
    }
}

pub type InvokeId = u32;

#[derive(Clone, PartialEq)]
/// #W3c says:
/// The \<invoke\> element is used to create an instance of an external service.
pub struct Invoke {
    pub doc_id: DocumentId,

    /// Platform Internal id
    pub id: InvokeId,

    /// #W3c says:
    /// Attribute 'id':\
    /// A string literal to be used as the identifier for this instance of <invoke>. See 3.14 IDs for details.
    pub external_id: String,

    /// #W3c says:
    /// Attribute 'idlocation':\
    /// Location expression.\
    /// Any data model expression evaluating to a data model location.\
    /// Must not occur with the 'id' attribute.
    pub external_id_location: String,

    /// #W3c says:
    /// Attribute 'type':\
    /// A URI specifying the type of the external service.
    pub type_name: String,

    /// #W3c says:
    /// Attribute 'typeexpr':\
    /// A dynamic alternative to 'type'. If this attribute is present, the SCXML Processor must evaluate it
    /// when the parent \<invoke\> element is evaluated and treat the result as if it had been entered as
    /// the value of 'type'.
    pub type_expr: String,

    /// #W3c says:
    /// List of valid location expressions
    pub namelist: Vec<String>,

    /// #W3c says:
    /// A URI to be passed to the external service.\
    /// Must not occur with the 'srcexpr' attribute or the \<content\> element.
    pub src: String,

    /// #W3c says:
    /// A dynamic alternative to 'src'. If this attribute is present,
    /// the SCXML Processor must evaluate it when the parent \<invoke\> element is evaluated and treat the result
    /// as if it had been entered as the value of 'src'.
    pub srcexpr: String,

    /// #W3c says:
    /// Boolean.\
    /// A flag indicating whether to forward events to the invoked process.
    pub autoforward: bool,

    /// content inside \<content\> child
    pub content: String,

    /// expr-attribute of \<content\> child
    pub content_expr: String,

    /// #W3c says:
    /// Executable content to massage the data returned from the invoked component. Occurs 0 or 1 times. See 6.5 \<finalize}> for details.
    pub finalize: ExecutableContentId,
}

impl Invoke {
    pub fn new() -> Invoke {
        Invoke {
            doc_id: 0,
            id: 0,
            external_id: "".to_string(),
            external_id_location: "".to_string(),
            type_name: "".to_string(),
            type_expr: "".to_string(),
            namelist: vec![],
            src: "".to_string(),
            srcexpr: "".to_string(),
            autoforward: false,
            content: "".to_string(),
            content_expr: "".to_string(),
            finalize: 0
        }
    }
}

impl Debug for Invoke {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Invoke")
            .field("platform id", &self.id)
            .field("id", &self.external_id)
            .field("idlocation", &self.external_id_location)
            .field("type", &self.type_name)
            .field("typeexpr", &self.type_expr)
            .field("src", &self.src)
            .field("srcexpr", &self.srcexpr)
            .field("autoforward", &self.autoforward)
            .field("content", &self.content)
            .field("content_expr", &self.content_expr)
            .finish()
    }
}


#[derive(Debug, Clone, PartialEq, Copy, Hash, Eq)]
pub enum Trace {
    METHODS,
    STATES,
    EVENTS,
    ARGUMENTS,
    RESULTS,
    ALL,
}

impl fmt::Display for Trace {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl FromStr for Trace {
    type Err = ();

    fn from_str(input: &str) -> Result<Trace, Self::Err> {
        match input.to_lowercase().as_str() {
            "methods" => Ok(Trace::METHODS),
            "states" => Ok(Trace::STATES),
            "events" => Ok(Trace::EVENTS),
            "arguments" => Ok(Trace::ARGUMENTS),
            "results" => Ok(Trace::RESULTS),
            "all" => Ok(Trace::ALL),
            _ => Err(()),
        }
    }
}

/// Trait used to trace methods and
/// states inside the FSM. What is traced can be controlled by
/// [Tracer::enableTrace] and [Tracer::disableTrace], see [Trace].
pub trait Tracer: Send + Debug {
    fn trace(&self, msg: &str);

    /// Enter a sub-scope, e.g. by increase the log indentation.
    fn enter(&self);

    /// Leave the current sub-scope, e.g. by decrease the log indentation.
    fn leave(&self);

    /// Enable traces for the specified scope.
    fn enableTrace(&mut self, flag: Trace);

    /// Disable traces for the specified scope.
    fn disableTrace(&mut self, flag: Trace);

    /// Return true if the given scape is enabled.
    fn isTrace(&self, flag: Trace) -> bool;

    /// Called by FSM if a method is entered
    fn enterMethod(&self, what: &str) {
        if self.isTrace(Trace::METHODS) {
            self.trace(format!(">>> {}", what).as_str());
            self.enter();
        }
    }

    /// Called by FSM if a method is exited
    fn exitMethod(&self, what: &str) {
        if self.isTrace(Trace::METHODS) {
            self.leave();
            self.trace(format!("<<< {}", what).as_str());
        }
    }

    /// Called by FSM if an internal event is send
    fn event_internal_send(&self, what: &Event) {
        if self.isTrace(Trace::EVENTS) {
            self.trace(format!("Int<- {} #{}", what.name, what.invokeid).as_str());
        }
    }

    /// Called by FSM if an internal event is received
    fn event_internal_received(&self, what: &Event) {
        if self.isTrace(Trace::EVENTS) {
            self.trace(format!("Int-> {} #{}", what.name, what.invokeid).as_str());
        }
    }

    /// Called by FSM if an external event is send
    fn event_external_send(&self, what: &Event) {
        if self.isTrace(Trace::EVENTS) {
            self.trace(format!("Ext<- {} #{}", what.name, what.invokeid).as_str());
        }
    }

    /// Called by FSM if an external event is received
    fn event_external_received(&mut self, what: &Event) {
        if what.name.starts_with("trace.") {
            let p = what.name.as_str().split('.').collect::<Vec<&str>>();
            if p.len() == 3 {
                match Trace::from_str(p.get(1).unwrap()) {
                    Ok(t) => {
                        match *p.get(2).unwrap() {
                            "on" | "ON" | "On" => {
                                self.enableTrace(t);
                            }
                            "off" | "OFF" | "Off" => {
                                self.disableTrace(t);
                            }
                            _ => {
                                self.trace(format!("Trace event '{}' with illegal flag '{}'. Use 'On' or 'Off'.", what.name, *p.get(2).unwrap()).as_str());
                            }
                        }
                    }
                    Err(_e) => {
                        self.trace(format!("Trace event '{}' has unknown trace flag '{}'", what.name, p.get(1).unwrap()).as_str());
                    }
                }
            }
        }
        if self.isTrace(Trace::EVENTS) {
            self.trace(format!("Ext-> {} #{}", what.name, what.invokeid).as_str());
        }
    }


    /// Called by FSM if a state is entered or left.
    fn traceState(&self, what: &str, s: &State) {
        if self.isTrace(Trace::STATES) {
            if s.name.is_empty() {
                self.trace(format!("{} #{}", what, s.id).as_str());
            } else {
                self.trace(format!("{} <{}> #{}", what, &s.name, s.id).as_str());
            }
        }
    }

    /// Called by FSM if a state is entered. Calls [traceState].
    fn traceEnterState(&self, s: &State) {
        self.traceState("Enter", s);
    }

    /// Called by FSM if a state is left. Calls [traceState].
    fn traceExitState(&self, s: &State) {
        self.traceState("Exit", s);
    }


    /// Called by FSM for input arguments in methods.
    fn traceArgument(&self, what: &str, d: &dyn Display) {
        if self.isTrace(Trace::ARGUMENTS) {
            self.trace(format!("In:{}:{}", what, d).as_str());
        }
    }

    /// Called by FSM for results in methods.
    fn traceResult(&self, what: &str, d: &dyn Display) {
        if self.isTrace(Trace::RESULTS) {
            self.trace(format!("Out:{}:{}", what, d).as_str());
        }
    }

    /// Helper method to trace a vector of ids.
    fn traceIdVec(&self, what: &str, l: &Vec<u32>) {
        self.trace(format!("{}: {}", what, vecToString(&l)).as_str());
    }

    /// Helper method to trace a OrderedSet of ids.
    fn traceIdSet(&self, what: &str, l: &OrderedSet<u32>) {
        self.trace(format!("{}: {}", what, vecToString(&l.data)).as_str());
    }
}

thread_local! {
    /// Trasce prefix for [DefaultTracer]
    static TRACE_PREFIX: RefCell<String> = RefCell::new("".to_string());
 }

#[derive(Debug)]
pub struct DefaultTracer {
    pub trace_flags: HashSet<Trace>,
}

impl Tracer for DefaultTracer {
    fn trace(&self, msg: &str) {
        info!("{}{}", DefaultTracer::get_prefix(), msg);
    }

    fn enter(&self) {
        let mut prefix = DefaultTracer::get_prefix();
        prefix += " ";
        DefaultTracer::set_prefix(prefix);
    }

    fn leave(&self) {
        let mut prefix = DefaultTracer::get_prefix();
        if prefix.len() > 0 {
            prefix.remove(0);
            DefaultTracer::set_prefix(prefix);
        }
    }

    fn enableTrace(&mut self, flag: Trace) {
        self.trace_flags.insert(flag);
    }

    fn disableTrace(&mut self, flag: Trace) {
        self.trace_flags.remove(&flag);
    }

    fn isTrace(&self, flag: Trace) -> bool {
        self.trace_flags.contains(&flag) || self.trace_flags.contains(&Trace::ALL)
    }
}

impl DefaultTracer {
    pub fn new() -> DefaultTracer {
        DefaultTracer {
            trace_flags: HashSet::new(),
        }
    }

    fn get_prefix() -> String {
        TRACE_PREFIX.with(|p| p.borrow().clone())
    }

    fn set_prefix(p: String) {
        TRACE_PREFIX.with(|pfx: &RefCell<String>| { *pfx.borrow_mut().deref_mut() = p; });
    }
}

/// #W3C says:
/// ##Global variables
/// The following variables are global from the point of view of the algorithm.
/// Their values will be set in the procedure interpret().
/// #Actual Implementation
/// In the W3C algorithm the datamodel is simple a global variable.
/// As the datamodel needs access to other global variables and rust doesn't like
/// accessing data of parents from inside a member, most global data is moved this
/// struct that is owned by the datamodel.
#[derive(Debug)]
pub struct GlobalData {
    pub configuration: OrderedSet<StateId>,
    pub statesToInvoke: OrderedSet<StateId>,
    pub historyValue: HashTable<StateId, OrderedSet<StateId>>,
    pub running: bool,

    pub internalQueue: Queue<Event>,
    pub externalQueue: BlockingQueue<Box<Event>>,
}

impl GlobalData {
    pub fn new() -> GlobalData {
        GlobalData {
            configuration: OrderedSet::new(),
            historyValue: HashTable::new(),
            running: false,
            statesToInvoke: OrderedSet::new(),
            internalQueue: Queue::new(),
            externalQueue: BlockingQueue::new(),
        }
    }
}


/// The FSM implementation, according to W3C proposal.
pub struct Fsm {
    pub tracer: Box<dyn Tracer>,
    pub datamodel: String,

    pub binding: BindingType,
    pub version: String,
    pub statesNames: StateNameMap,
    pub executableContent: HashMap<ExecutableContentId, Vec<Box<dyn ExecutableContent>>>,

    pub name: String,

    /// A FSM can have actual multiple initial-target-states, so this state may be artificial.
    /// Reader has to generate a parent state if needed.
    /// This state also serve as the "scxml" state element were mentioned.
    pub pseudo_root: StateId,

    /**
     * The only real storage of states, identified by the Id
     * If a state has no declared id, one is generated.
     */
    pub states: Vec<State>,
    pub transitions: TransitionMap,

    pub script: ExecutableContentId,

    pub caller_invoke_id: InvokeId,
    pub caller_sender: Option<Sender<Box<Event>>>,

    pub timer: timer::Timer,
}

impl Debug for Fsm {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Fsm{{v:{} root:{} states:", self.version, self.pseudo_root)?;
        display_state_map(&self.states, f)?;
        display_transition_map(&self.transitions, f)?;
        write!(f, "}}")
    }
}

fn display_state_map(sm: &StateVec, f: &mut Formatter<'_>) -> std::fmt::Result {
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

fn display_transition_map(sm: &TransitionMap, f: &mut Formatter<'_>) -> std::fmt::Result {
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
            tracer: Box::new(DefaultTracer::new()),
            caller_invoke_id: 0,
            caller_sender: None,
            name: "FSM".to_string(),
            script: 0,
            version: "1.0".to_string(),
            binding: BindingType::Early,
            statesNames: StateNameMap::new(),
            executableContent: HashMap::new(),
            timer: timer::Timer::new(),
        }
    }

    pub fn get_state_by_name(&self, name: &Name) -> &State
    {
        self.get_state_by_id(*self.statesNames.get(name).unwrap())
    }

    pub fn get_state_by_name_mut(&mut self, name: &Name) -> &mut State
    {
        self.get_state_by_id_mut(*self.statesNames.get(name).unwrap())
    }

    /// Gets a state by id.
    /// The id MUST exists.
    pub fn get_state_by_id(&self, state_id: StateId) -> &State
    {
        self.states.get((state_id - 1) as usize).unwrap()
    }

    /// Gets a mutable state by id.
    /// The id MUST exists.
    pub fn get_state_by_id_mut(&mut self, state_id: StateId) -> &mut State
    {
        self.states.get_mut((state_id - 1) as usize).unwrap()
    }

    pub fn get_transition_by_id_mut(&mut self, transition_id: TransitionId) -> &mut Transition
    {
        self.transitions.get_mut(&transition_id).unwrap()
    }

    pub fn get_transition_by_id(&self, transition_id: TransitionId) -> &Transition
    {
        self.transitions.get(&transition_id).unwrap()
    }

    fn stateDocumentOrder(&self, sid1: &StateId, sid2: &StateId) -> ::std::cmp::Ordering {
        // TODO: Optimize! Do that state-ids == index in fsm.states.
        let s1 = self.get_state_by_id(*sid1);
        let s2 = self.get_state_by_id(*sid2);

        if s1.doc_id > s2.doc_id {
            std::cmp::Ordering::Greater
        } else if s1.doc_id == s2.doc_id {
            std::cmp::Ordering::Equal
        } else { std::cmp::Ordering::Less }
    }

    fn stateEntryOrder(&self, s1: &StateId, s2: &StateId) -> ::std::cmp::Ordering {
        // Same as Document order
        self.stateDocumentOrder(s1, s2)
    }

    fn stateExitOrder(&self, s1: &StateId, s2: &StateId) -> ::std::cmp::Ordering {
        // Same as Document order
        self.stateDocumentOrder(s1, s2)
    }

    fn transitionDocumentOrder(&self, t1: &&Transition, t2: &&Transition) -> ::std::cmp::Ordering {
        if t1.doc_id > t2.doc_id {
            std::cmp::Ordering::Greater
        } else if t1.doc_id == t2.doc_id {
            std::cmp::Ordering::Equal
        } else { std::cmp::Ordering::Less }
    }

    fn invokeDocumentOrder(s1: &Invoke, s2: &Invoke) -> ::std::cmp::Ordering {
        if s1.id > s2.id {
            std::cmp::Ordering::Greater
        } else if s1.id == s2.id {
            std::cmp::Ordering::Equal
        } else { std::cmp::Ordering::Less }
    }

    /// #W3C says:
    /// The purpose of this procedure is to initialize the interpreter and to start processing.
    ///
    /// In order to interpret an SCXML document, first (optionally) perform
    /// [xinclude](https://www.w3.org/TR/scxml/#xinclude) processing and (optionally) validate
    /// the document, throwing an exception if validation fails.
    /// Then convert initial attributes to \<initial\> container children with transitions
    /// to the state specified by the attribute. (This step is done purely to simplify the statement of
    /// the algorithm and has no effect on the system's behavior.
    ///
    /// Such transitions will not contain any executable content).
    /// Initialize the global data structures, including the data model.
    /// If binding is set to 'early', initialize the data model.
    /// Then execute the global \<script\> element, if any.
    /// Finally call enterStates on the initial configuration, set the global running
    /// variable to true and start the interpreter's event loop.
    /// ```
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
        self.tracer.enterMethod("interpret");
        if !self.valid() {
            self.failWithError()
        }
        self.expandScxmlSource();
        {
            datamodel.clear();
            {
                let global = datamodel.global();
                global.internalQueue.clear();
                global.historyValue.clear();
                global.running = true;
            }
            if self.binding == BindingType::Early {
                datamodel.initializeDataModel(self, self.pseudo_root);
            }
        }
        self.executeGlobalScriptElement(datamodel);

        let mut initalStates = List::new();
        let itid = self.get_state_by_id(self.pseudo_root).initial;
        if itid != 0 {
            initalStates.push(itid);
        }
        self.enterStates(datamodel, &initalStates);
        self.mainEventLoop(datamodel);
        self.tracer.exitMethod("interpret");
    }


    /// #Actual implementation:
    /// TODO
    /// * check if all state/transition references are correct (all states have a document-id)
    /// * check if all special scxml conditions are satisfied.
    fn valid(&self) -> bool {
        for state in &self.states {
            if state.doc_id == 0 {
                self.tracer.trace(&format!("Referenced state '{}' is not declared", state.name).as_str());
                return false;
            }
        }
        true
    }

    /// #Actual implementation:
    /// Throws a panic
    fn failWithError(&self) {
        panic!("FSM has failed");
    }


    /// #Actual implementation:
    /// This method is called on the fsm model, after
    /// the xml document was processed. It should check if all References to states are fulfilled.
    /// After this method all "StateId" or "TransactionId" shall be valid and have to lead to a panic.
    fn expandScxmlSource(&mut self) {}

    fn executeGlobalScriptElement(&mut self, datamodel: &mut dyn Datamodel) {
        self.tracer.enterMethod("executeGlobalScriptElement");
        if self.script != 0 {
            datamodel.executeContent(self, self.script);
        }
        self.tracer.exitMethod("executeGlobalScriptElement");
    }

    /// #W3C says:
    /// ## procedure mainEventLoop()
    /// This loop runs until we enter a top-level final state or an external entity cancels processing.
    /// In either case 'running' will be set to false (see EnterStates, below, for termination by
    /// entering a top-level final state.)
    ///
    /// At the top of the loop, we have either just entered the state machine, or we have just
    /// processed an external event. Each iteration through the loop consists of four main steps:
    /// 1) Complete the macrostep by repeatedly taking any internally enabled transitions, namely
    /// those that don't require an event or that are triggered by an internal event.
    /// After each such transition/microstep, check to see if we have reached a final state.
    /// 2) When there are no more internally enabled transitions available, the macrostep is done.
    /// Execute any \<invoke\> tags for states that we entered on the last iteration through the loop
    /// 3) If any internal events have been generated by the invokes, repeat step 1 to handle any
    /// errors raised by the \<invoke\> elements.
    /// 4) When the internal event queue is empty, wait for
    /// an external event and then execute any transitions that it triggers. However special
    /// preliminary processing is applied to the event if the state has executed any \<invoke\>
    /// elements. First, if this event was generated by an invoked process, apply \<finalize\>
    /// processing to it. Secondly, if any \<invoke\> elements have autoforwarding set, forward the
    /// event to them. These steps apply before the transitions are taken.
    ///
    /// This event loop thus enforces run-to-completion semantics, in which the system process an external event and then takes all the 'follow-up' transitions that the processing has enabled before looking for another external event. For example, suppose that the external event queue contains events ext1 and ext2 and the machine is in state s1. If processing ext1 takes the machine to s2 and generates internal event int1, and s2 contains a transition t triggered by int1, the system is guaranteed to take t, no matter what transitions s2 or other states have that would be triggered by ext2. Note that this is true even though ext2 was already in the external event queue when int1 was generated. In effect, the algorithm treats the processing of int1 as finishing up the processing of ext1.
    /// ```
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
    ///         # either we're in a final state, and we break out of the loop
    ///         if not running:
    ///             break
    ///         # or we've completed a macrostep, so we start a new macrostep by waiting for an external event
    ///         # Here we invoke whatever needs to be invoked. The implementation of 'invoke' is platform-specific
    ///         for state in statesToInvoke.sort(entryOrder):
    ///             for inv in state.invoke.sort(documentOrder):
    ///                 invoke(inv)
    ///         statesToInvoke.clear()
    ///         # Invoking may have raised internal error events and we iterate to handle them
    ///         if not internalQueue.isEmpty():
    ///             continue
    ///         # A blocking wait for an external event.  Alternatively, if we have been invoked
    ///         # our parent session also might cancel us.  The mechanism for this is platform specific,
    ///         # but here we assume it’s a special event we receive
    ///         externalEvent = externalQueue.dequeue()
    ///         if isCancelEvent(externalEvent):
    ///             running = false
    ///             continue
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
    fn mainEventLoop(&mut self, datamodel: &mut dyn Datamodel) {
        self.tracer.enterMethod("mainEventLoop");
        while datamodel.global().running {
            let mut enabledTransitions;
            let mut macrostepDone = false;
            // Here we handle eventless transitions and transitions
            // triggered by internal events until macrostep is complete
            while datamodel.global().running && !macrostepDone {
                enabledTransitions = self.selectEventlessTransitions(datamodel);
                if enabledTransitions.isEmpty() {
                    if datamodel.global().internalQueue.isEmpty() {
                        macrostepDone = true;
                    } else {
                        self.tracer.enterMethod("internalQueue.dequeue");

                        let internalEvent =
                            {
                                datamodel.global().internalQueue.dequeue()
                            };
                        self.tracer.exitMethod("internalQueue.dequeue");
                        self.tracer.event_internal_received(&internalEvent);
                        datamodel.set(&"_event".to_string(), internalEvent.get_copy());
                        enabledTransitions = self.selectTransitions(datamodel, &internalEvent);
                    }
                }
                if !enabledTransitions.isEmpty() {
                    self.microstep(datamodel, &enabledTransitions.toList())
                }
            }
            // either we're in a final state, and we break out of the loop
            if !datamodel.global().running {
                break;
            }
            // or we've completed a macrostep, so we start a new macrostep by waiting for an external event
            // Here we invoke whatever needs to be invoked. The implementation of 'invoke' is platform-specific
            for sid in datamodel.global().statesToInvoke.sort(
                &|s1, s2| { self.stateEntryOrder(s1, s2) }).iterator()
            {
                let state = self.get_state_by_id(*sid);
                for inv in state.invoke.sort(&|i1, i2| { Fsm::invokeDocumentOrder(i1, i2) }).iterator() {
                    self.invoke(inv);
                }
            }

            let externalEvent;
            {
                let gdb = datamodel.global();
                gdb.statesToInvoke.clear();
                // Invoking may have raised internal error events and we iterate to handle them
                if !gdb.internalQueue.isEmpty() {
                    continue;
                }
                // A blocking wait for an external event.  Alternatively, if we have been invoked
                // our parent session also might cancel us.  The mechanism for this is platform specific,
                // but here we assume it’s a special event we receive
                self.tracer.enterMethod("externalQueue.dequeue");
                externalEvent = gdb.externalQueue.dequeue();
                self.tracer.exitMethod("externalQueue.dequeue");
                self.tracer.event_external_received(&externalEvent);
                if self.isCancelEvent(&externalEvent) {
                    gdb.running = false;
                    continue;
                }
            }
            let mut toFinalize: Vec<InvokeId> = Vec::new();
            let mut toForward: Vec<InvokeId> = Vec::new();
            {
                let invokeId = externalEvent.invokeid;
                datamodel.set(&"_event".to_string(), externalEvent.get_copy());
                for sid in datamodel.global().configuration.iterator() {
                    let state = self.get_state_by_id(*sid);
                    for inv in state.invoke.iterator() {
                        if inv.id == invokeId {
                            toFinalize.push(inv.id);
                        }
                        if inv.autoforward {
                            toForward.push(inv.id);
                        }
                    }
                }
            }
            for invId in toFinalize {
                self.applyFinalize(invId, &externalEvent);
            }
            for invId in toForward {
                self.send(invId, &externalEvent);
            }

            enabledTransitions = self.selectTransitions(datamodel, &externalEvent);
            if !enabledTransitions.isEmpty() {
                self.microstep(datamodel, &enabledTransitions.toList());
            }
        }
        // End of outer while running loop.  If we get here, we have reached a top-level final state or have been cancelled
        self.exitInterpreter(datamodel);
        self.tracer.exitMethod("mainEventLoop");
    }

    /// #W3C says:
    /// # procedure exitInterpreter()
    /// The purpose of this procedure is to exit the current SCXML process by exiting all active
    /// states. If the machine is in a top-level final state, a Done event is generated.
    /// (Note that in this case, the final state will be the only active state.)
    /// The implementation of returnDoneEvent is platform-dependent, but if this session is the
    /// result of an \<invoke\> in another SCXML session, returnDoneEvent will cause the event
    /// done.invoke.\<id\> to be placed in the external event queue of that session, where \<id\> is
    /// the id generated in that session when the \<invoke\> was executed.
    /// ```
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
    fn exitInterpreter(&mut self, datamodel: &mut dyn Datamodel) {
        let statesToExit = datamodel.global().configuration.toList().sort(
            &|s1, s2| { self.stateExitOrder(s1, s2) });
        for sid in statesToExit.iterator() {
            let mut content: Vec<ExecutableContentId> = Vec::new();
            let mut invokes: Vec<Invoke> = Vec::new();
            {
                let s = self.get_state_by_id(*sid);
                if s.onexit != 0 {
                    content.push(s.onexit);
                }
                for inv in s.invoke.iterator() {
                    invokes.push(inv.clone());
                }
            }
            for ct in content {
                self.executeContent(datamodel, ct);
            }
            for inv in invokes {
                self.cancelInvoke(&inv)
            }

            datamodel.global().configuration.delete(sid);
            {
                let s = self.get_state_by_id(*sid);
                if self.isFinalState(s) && self.isSCXMLElement(s.parent) {
                    self.returnDoneEvent(&s.donedata.clone());
                }
            }
        }
    }

    /// #W3C says:
    /// The implementation of returnDoneEvent is platform-dependent, but if this session is the
    /// result of an \<invoke\> in another SCXML session, returnDoneEvent will cause the event
    /// done.invoke.\<id\> to be placed in the external event queue of that session, where \<id\> is
    /// the id generated in that session when the \<invoke\> was executed.
    fn returnDoneEvent(&mut self, done_data: &Option<DoneData>) {
        // TODO. Currently no "sender" is set by the calling code.
        if self.caller_invoke_id != 0 {
            match &self.caller_sender {
                None => panic!("caller-sender not available but caller-invoke-id is set."),
                Some(sender) => {
                    match sender.send(Box::new(Event::new("done.invoke.", &self.caller_invoke_id.to_string(), done_data))) {
                        Err(e) => {
                            self.tracer.trace(format!("Failed to send 'done.invoke'. {}", e).as_str());
                        }
                        _ => {}
                    }
                }
            }
        }
    }


    /// #W3C says:
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
    /// ```
    ///
    /// function selectEventlessTransitions():
    ///     enabledTransitions = new OrderedSet()
    ///     atomicStates = configuration.toList().filter(isAtomicState).sort(documentOrder)
    ///     for state in atomicStates:
    ///         loop: for s in [state].append(getProperAncestors(state, null)):
    ///             for t in s.transition.sort(documentOrder):
    ///                 if not t.event and conditionMatch(t):
    ///                     enabledTransitions.add(t)
    ///                     break loop
    ///     enabledTransitions = removeConflictingTransitions(enabledTransitions)
    ///     return enabledTransitions
    /// ```
    fn selectEventlessTransitions(&mut self, datamodel: &mut dyn Datamodel) -> OrderedSet<TransitionId> {
        self.tracer.enterMethod("selectEventlessTransitions");

        let mut enabledTransitions: OrderedSet<TransitionId> = OrderedSet::new();
        let atomicStates = datamodel.global().configuration.toList()
            .filterBy(&|sid| -> bool { self.isAtomicState(self.get_state_by_id(*sid)) })
            .sort(&|s1, s2| { self.stateDocumentOrder(s1, s2) });
        self.tracer.traceArgument("atomicStates", &atomicStates);
        for sid in atomicStates.iterator() {
            let mut states: List<StateId> = List::new();
            states.push(*sid);
            states.appendSet(&self.getProperAncestors(*sid, 0));
            let mut condT = Vec::new();
            for s in states.iterator() {
                let state = self.get_state_by_id(*s);
                for t in self.to_transition_list(&state.transitions)
                    .sort(&|t1: &&Transition, t2: &&Transition| { self.transitionDocumentOrder(t1, t2) }).iterator() {
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
        self.tracer.traceResult("enabledTransitions", &enabledTransitions);
        self.tracer.exitMethod("selectEventlessTransitions");
        enabledTransitions
    }

    /// #W3C says:
    /// function selectTransitions(event)
    /// The purpose of the selectTransitions()procedure is to collect the transitions that are enabled by this event in the current configuration.
    ///
    /// Create an empty set of enabledTransitions. For each atomic state , find a transition whose 'event' attribute matches event and whose condition evaluates to true. If multiple matching transitions are present, take the first in document order. If none are present, search in the state's ancestors in ancestry order until one is found. As soon as such a transition is found, add it to enabledTransitions, and proceed to the next atomic state in the configuration. If no such transition is found in the state or its ancestors, proceed to the next state in the configuration. When all atomic states have been visited and transitions selected, filter out any preempted transitions and return the resulting set.
    /// ```
    /// function selectTransitions(event):
    ///     enabledTransitions = new OrderedSet()
    ///     atomicStates = configuration.toList().filter(isAtomicState).sort(documentOrder)
    ///     for state in atomicStates:
    ///         loop: for s in [state].append(getProperAncestors(state, null)):
    ///             for t in s.transition.sort(documentOrder):
    ///                 if t.event and nameMatch(t.event, event.name) and conditionMatch(t):
    ///                     enabledTransitions.add(t)
    ///                     break loop
    ///     enabledTransitions = removeConflictingTransitions(enabledTransitions)
    ///     return enabledTransitions
    /// ```
    fn selectTransitions(&mut self, datamodel: &mut dyn Datamodel, event: &Event) -> OrderedSet<TransitionId> {
        self.tracer.enterMethod("selectTransitions");
        let mut enabledTransitions: OrderedSet<TransitionId> = OrderedSet::new();
        let atomicStates = datamodel.global().configuration.toList()
            .filterBy(&|sid| -> bool { self.isAtomicStateId(sid) }).sort(
            &|s1, s2| { self.stateDocumentOrder(s1, s2) });
        for state in atomicStates.iterator() {
            let mut condT = Vec::new();
            for sid in List::from_array(&[*state])
                .appendSet(&self.getProperAncestors(*state, 0)).iterator() {
                let s = self.get_state_by_id(*sid);
                let mut transition: Vec<&Transition> = Vec::new();
                for tid in s.transitions.iterator() {
                    transition.push(self.get_transition_by_id(*tid));
                }

                transition.sort_by(&|t1: &&Transition, t2: &&Transition| { self.transitionDocumentOrder(t1, t2) });
                for t in transition {
                    if (!t.events.is_empty()) && self.nameMatch(&t.events, &event.name) {
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
        self.tracer.traceResult("enabledTransitions", &enabledTransitions);
        self.tracer.exitMethod("selectTransitions");
        enabledTransitions
    }


    /// #W3C says:
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
    /// We start with a list of enabledTransitions and produce a conflict-free list of filteredTransitions. For each t1 in enabledTransitions, we test it against all t2 that are already selected in filteredTransitions. If there is a conflict, then if t1's source state is a descendant of t2's source state, we prefer t1 and say that it preempts t2 (so we we make a note to remove t2 from filteredTransitions). Otherwise, we prefer t2 since it was selected in an earlier state in document order, so we say that it preempts t1. (There's no need to do anything in this case since t2 is already in filteredTransitions. Furthermore, once one transition preempts t1, there is no need to test t1 against any other transitions.) Finally, if t1 isn't preempted by any transition in filteredTransitions, remove any transitions that it preempts and add it to that list.
    /// ```
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
    ///         if not t1Preempted:
    ///             for t3 in transitionsToRemove.toList():
    ///                 filteredTransitions.delete(t3)
    ///             filteredTransitions.add(t1)
    ///
    ///     return filteredTransitions
    /// ```
    fn removeConflictingTransitions(&self, datamodel: &mut dyn Datamodel, enabledTransitions: &OrderedSet<TransitionId>) -> OrderedSet<TransitionId> {
        let mut filteredTransitions: OrderedSet<TransitionId> = OrderedSet::new();
        //toList sorts the transitions in the order of the states that selected them
        for tid1 in enabledTransitions.toList().iterator() {
            let t1 = self.get_transition_by_id(*tid1);
            let mut t1Preempted = false;
            let mut transitionsToRemove = OrderedSet::new();
            let filteredTransitionList = filteredTransitions.toList();
            for tid2 in filteredTransitionList.iterator()
            {
                if self.computeExitSet(datamodel, &List::from_array(&[*tid1])).hasIntersection(&self.computeExitSet(datamodel, &List::from_array(&[*tid2]))) {
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


    /// #W3C says:
    /// # procedure microstep(enabledTransitions)
    /// The purpose of the microstep procedure is to process a single set of transitions. These may have been enabled by an external event, an internal event, or by the presence or absence of certain values in the data model at the current point in time. The processing of the enabled transitions must be done in parallel ('lock step') in the sense that their source states must first be exited, then their actions must be executed, and finally their target states entered.
    ///
    /// If a single atomic state is active, then enabledTransitions will contain only a single transition. If multiple states are active (i.e., we are in a parallel region), then there may be multiple transitions, one per active atomic state (though some states may not select a transition.) In this case, the transitions are taken in the document order of the atomic states that selected them.
    /// ```
    /// procedure microstep(enabledTransitions):
    ///     exitStates(enabledTransitions)
    ///     executeTransitionContent(enabledTransitions)
    ///     enterStates(enabledTransitions)
    /// ```
    fn microstep(&mut self, datamodel: &mut dyn Datamodel, enabledTransitions: &List<TransitionId>) {
        self.tracer.enterMethod("microstep");
        self.exitStates(datamodel, enabledTransitions);
        self.executeTransitionContent(datamodel, enabledTransitions);
        self.enterStates(datamodel, enabledTransitions);
        self.tracer.exitMethod("microstep");
    }


    /// #W3C says:
    /// # procedure exitStates(enabledTransitions)
    /// Compute the set of states to exit. Then remove all the states on statesToExit from the set of states that will have invoke processing done at the start of the next macrostep. (Suppose macrostep M1 consists of microsteps m11 and m12. We may enter state s in m11 and exit it in m12. We will add s to statesToInvoke in m11, and must remove it in m12. In the subsequent macrostep M2, we will apply invoke processing to all states that were entered, and not exited, in M1.) Then convert statesToExit to a list and sort it in exitOrder.
    ///
    /// For each state s in the list, if s has a deep history state h, set the history value of h to be the list of all atomic descendants of s that are members in the current configuration, else set its value to be the list of all immediate children of s that are members of the current configuration. Again for each state s in the list, first execute any onexit handlers, then cancel any ongoing invocations, and finally remove s from the current configuration.
    ///
    /// ```
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
    fn exitStates(&mut self, datamodel: &mut dyn Datamodel, enabledTransitions: &List<TransitionId>) {
        self.tracer.enterMethod("exitStates");

        let statesToExit = self.computeExitSet(datamodel, enabledTransitions);
        for s in statesToExit.iterator() {
            datamodel.global().statesToInvoke.delete(s);
        }
        let statesToExitSorted = statesToExit.sort(
            &|s1, s2| { self.stateExitOrder(s1, s2) });
        let mut ahistory: HashTable<StateId, OrderedSet<StateId>> = HashTable::new();

        let configStateList = self.set_to_state_list(&datamodel.global().configuration);

        for sid in statesToExitSorted.iterator() {
            let s = self.get_state_by_id(*sid);
            for hid in s.history.iterator() {
                let h = self.get_state_by_id(*hid);
                if h.history_type == HistoryType::Deep
                {
                    let stateIdList = self
                        .state_list_to_id_set(&configStateList.filterBy(
                            &|s0| -> bool { self.isAtomicState(*s0) && self.isDescendant(s0.id, s.id) }));
                    ahistory.putMove(h.id, stateIdList);
                } else {
                    ahistory.put(h.id, &datamodel.global().configuration.toList().filterBy(
                        &|s0| -> bool { self.get_state_by_id(*s0).parent == s.id }).toSet());
                }
            }
        }

        datamodel.global().historyValue.putAll(&ahistory);

        for sid in statesToExitSorted.iterator() {
            let mut exe: List<ExecutableContentId> = List::new();
            {
                let s = self.get_state_by_id(*sid);
                self.tracer.traceExitState(s);
                exe.push(s.onexit);
            }

            for content in exe.iterator() {
                self.executeContent(datamodel, *content);
            }

            let mut invokeList: List<InvokeId> = List::new();
            {
                let s = self.get_state_by_id(*sid);
                for inv in s.invoke.iterator() {
                    invokeList.push(inv.id);
                }
            }

            for invokeId in invokeList.iterator() {
                self.cancelInvokeId(*invokeId);
            }

            datamodel.global().configuration.delete(sid)
        }
        self.tracer.exitMethod("exitStates");
    }


    /// #W3C says:
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
    /// ```
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
    fn enterStates(&mut self, datamodel: &mut dyn Datamodel, enabledTransitions: &List<StateId>) {
        self.tracer.enterMethod("enterStates");
        let binding = self.binding;
        let mut statesToEnter = OrderedSet::new();
        let mut statesForDefaultEntry = OrderedSet::new();

        // initialize the temporary table for default content in history states
        let mut defaultHistoryContent: HashTable<StateId, ExecutableContentId> = HashTable::new();
        self.computeEntrySet(datamodel, enabledTransitions, &mut statesToEnter, &mut statesForDefaultEntry, &mut defaultHistoryContent);
        for s in statesToEnter.toList().sort(&|s1, s2| { self.stateEntryOrder(s1, s2) }).iterator() {
            {
                self.tracer.traceEnterState(&self.get_state_by_id(*s));
            }
            {
                datamodel.global().configuration.add(*s);
                datamodel.global().statesToInvoke.add(*s);
            }
            let mut toInit: StateId = 0;
            {
                let stateS: &mut State = self.get_state_by_id_mut(*s);
                if binding == BindingType::Late && stateS.isFirstEntry {
                    toInit = *s;
                    stateS.isFirstEntry = false;
                }
            }
            if toInit != 0 {
                datamodel.initializeDataModel(self, toInit);
            }
            let mut exe = Vec::new();
            {
                let stateS: &State = self.get_state_by_id(*s);
                if stateS.onentry != 0 {
                    exe.push(stateS.onentry);
                }
                if statesForDefaultEntry.isMember(&s) {
                    let stateS: &State = self.get_state_by_id(*s);
                    if stateS.initial > 0 {
                        exe.push(self.get_transition_by_id(stateS.initial).content);
                    }
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
                let stateS = self.get_state_by_id(*s);
                let parent: StateId = stateS.parent;
                if self.isSCXMLElement(parent) {
                    datamodel.global().running = false;
                } else {
                    let parentS = self.get_state_by_id(parent);
                    self.enqueue_internal(datamodel, Event::new("done.state.", &parentS.name, &stateS.donedata));
                    let stateParent = self.get_state_by_id(parent);
                    let grandparent: StateId = stateParent.parent;
                    if self.isParallelState(grandparent) {
                        if self.getChildStates(grandparent).every(
                            &|s: &StateId| -> bool{ self.isInFinalState(datamodel, *s) }) {
                            let grandparentS = self.get_state_by_id(grandparent);
                            self.enqueue_internal(datamodel, Event::new("done.state.", &grandparentS.name, &None));
                        }
                    }
                }
            }
        }
        self.tracer.exitMethod("enterStates");
    }

    /// Put an event into the internal queue.
    pub fn enqueue_internal(&mut self, datamodel: &mut dyn Datamodel, event: Event) {
        self.tracer.event_internal_send(&event);
        datamodel.global().internalQueue.enqueue(event);
    }

    pub fn executeContent(&mut self, datamodel: &mut dyn Datamodel, contentId: ExecutableContentId) {
        self.tracer.enterMethod("executeContent");
        self.tracer.traceArgument("contentId", &contentId);
        if contentId != 0 {
            datamodel.executeContent(self, contentId);
        }
        self.tracer.exitMethod("executeContent");
    }

    pub fn isParallelState(&self, state: StateId) -> bool {
        self.tracer.enterMethod("isParallelState");
        self.tracer.traceArgument("state", &state);
        let b = state > 0 && self.get_state_by_id(state).is_parallel;
        self.tracer.traceResult("parallel", &b.to_string());
        self.tracer.exitMethod("isParallelState");
        b
    }

    pub fn isSCXMLElement(&self, state: StateId) -> bool {
        state == self.pseudo_root
    }

    pub fn isFinalState(&self, state: &State) -> bool {
        state.is_final
    }

    pub fn isFinalStateId(&self, state: StateId) -> bool {
        self.isFinalState(self.get_state_by_id(state))
    }

    pub fn isAtomicState(&self, state: &State) -> bool {
        state.states.is_empty()
    }

    pub fn isAtomicStateId(&self, sid: &StateId) -> bool {
        self.isAtomicState(self.get_state_by_id(*sid))
    }


    /// #W3C says:
    /// # procedure computeExitSet(enabledTransitions)
    /// For each transition t in enabledTransitions, if t is targetless then do nothing, else compute the transition's domain.
    /// (This will be the source state in the case of internal transitions) or the least common compound ancestor
    /// state of the source state and target states of t (in the case of external transitions. Add to the statesToExit
    /// set all states in the configuration that are descendants of the domain.
    /// ```
    /// function computeExitSet(transitions)
    ///     statesToExit = new OrderedSet
    ///     for t in transitions:
    ///         if t.target:
    ///             domain = getTransitionDomain(t)
    ///             for s in configuration:
    ///                 if isDescendant(s,domain):
    ///                     statesToExit.add(s)
    ///     return statesToExit
    /// ```
    fn computeExitSet(&self, datamodel: &mut dyn Datamodel, transitions: &List<TransitionId>) -> OrderedSet<StateId> {
        self.tracer.enterMethod("computeExitSet");
        self.tracer.traceArgument("transitions", &transitions);
        let mut statesToExit: OrderedSet<StateId> = OrderedSet::new();
        for tid in transitions.iterator() {
            let t = self.get_transition_by_id(*tid);
            if !t.target.is_empty() {
                let domain = self.getTransitionDomain(datamodel, t);
                for s in datamodel.global().configuration.iterator() {
                    if self.isDescendant(*s, domain) {
                        statesToExit.add(*s);
                    }
                }
            }
        }
        self.tracer.traceResult("statesToExit", &statesToExit);
        self.tracer.exitMethod("computeExitSet");
        statesToExit
    }

    /// #W3C says:
    /// # procedure executeTransitionContent(enabledTransitions)
    /// For each transition in the list of enabledTransitions, execute its executable content.
    /// ```
    /// procedure executeTransitionContent(enabledTransitions):
    ///     for t in enabledTransitions:
    ///         executeContent(t)
    /// ```
    fn executeTransitionContent(&mut self, datamodel: &mut dyn Datamodel, enabledTransitions: &List<TransitionId>) {
        for tid in enabledTransitions.iterator() {
            let t = self.get_transition_by_id(*tid);
            if t.content > 0 {
                self.executeContent(datamodel, t.content);
            }
        }
    }

    /// #W3C says:
    /// # procedure computeEntrySet(transitions, statesToEnter, statesForDefaultEntry, defaultHistoryContent)
    /// Compute the complete set of states that will be entered as a result of taking 'transitions'.
    /// This value will be returned in 'statesToEnter' (which is modified by this procedure). Also
    /// place in 'statesForDefaultEntry' the set of all states whose default initial states were
    /// entered. First gather up all the target states in 'transitions'. Then add them and, for all
    /// that are not atomic states, add all of their (default) descendants until we reach one or
    /// more atomic states. Then add any ancestors that will be entered within the domain of the
    /// transition. (Ancestors outside of the domain of the transition will not have been exited.)
    /// ```
    /// procedure computeEntrySet(transitions, statesToEnter, statesForDefaultEntry, defaultHistoryContent)
    ///     for t in transitions:
    ///         for s in t.target:
    ///             addDescendantStatesToEnter(s,statesToEnter,statesForDefaultEntry, defaultHistoryContent)
    ///         ancestor = getTransitionDomain(t)
    ///         for s in getEffectiveTargetStates(t)):
    ///             addAncestorStatesToEnter(s, ancestor, statesToEnter, statesForDefaultEntry, defaultHistoryContent)
    /// ```
    fn computeEntrySet(&mut self, datamodel: &mut dyn Datamodel, transitions: &List<TransitionId>, statesToEnter: &mut OrderedSet<StateId>,
                       statesForDefaultEntry: &mut OrderedSet<StateId>, defaultHistoryContent: &mut HashTable<StateId, ExecutableContentId>) {
        self.tracer.enterMethod("computeEntrySet");
        self.tracer.traceArgument("transitions", transitions);

        for tid in transitions.iterator() {
            let t = self.get_transition_by_id(*tid);
            for s in t.target.iter() {
                self.addDescendantStatesToEnter(datamodel, *s, statesToEnter, statesForDefaultEntry, defaultHistoryContent);
            }
            let ancestor = self.getTransitionDomain(datamodel, t);
            for s in self.getEffectiveTargetStates(datamodel, t).iterator() {
                self.addAncestorStatesToEnter(datamodel, *s, ancestor, statesToEnter, statesForDefaultEntry, defaultHistoryContent);
            }
        }
        self.tracer.traceResult("statesToEnter>", statesToEnter);
        self.tracer.exitMethod("computeEntrySet");
    }

    /// #W3C says:
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
    /// ```
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
    fn addDescendantStatesToEnter(&self, datamodel: &mut dyn Datamodel, sid: StateId, statesToEnter: &mut OrderedSet<StateId>,
                                  statesForDefaultEntry: &mut OrderedSet<StateId>, defaultHistoryContent: &mut HashTable<StateId, ExecutableContentId>) {
        self.tracer.enterMethod("addDescendantStatesToEnter");
        self.tracer.traceArgument("State", &sid);

        let state = self.get_state_by_id(sid);
        if self.isHistoryState(sid) {
            if datamodel.global().historyValue.has(sid) {
                let mut stateIds: Vec<StateId> = Vec::new();
                for s in datamodel.global().historyValue.get(sid).iterator()
                {
                    stateIds.push(*s);
                }
                for s in &stateIds {
                    self.addDescendantStatesToEnter(datamodel, *s, statesToEnter, statesForDefaultEntry, defaultHistoryContent);
                }
                for s in &stateIds {
                    self.addAncestorStatesToEnter(datamodel, *s, state.parent, statesToEnter, statesForDefaultEntry, defaultHistoryContent);
                }
            } else {
                // A history state have exactly one transition which specified the default history configuration.
                let defaultTransition = self.get_transition_by_id(*state.transitions.head());
                defaultHistoryContent.put(state.parent, &defaultTransition.content);
                for s in &defaultTransition.target {
                    self.addDescendantStatesToEnter(datamodel, *s, statesToEnter, statesForDefaultEntry, defaultHistoryContent);
                }
                for s in &defaultTransition.target {
                    self.addAncestorStatesToEnter(datamodel, *s, state.parent, statesToEnter, statesForDefaultEntry, defaultHistoryContent);
                }
            }
        } else {
            statesToEnter.add(sid);
            if self.isCompoundState(sid) {
                statesForDefaultEntry.add(sid);
                if state.initial != 0 {
                    let initialTransition = self.get_transition_by_id(state.initial);
                    for s in &initialTransition.target {
                        self.addDescendantStatesToEnter(datamodel, *s, statesToEnter, statesForDefaultEntry, defaultHistoryContent);
                    }
                    for s in &initialTransition.target {
                        self.addAncestorStatesToEnter(datamodel, *s, sid, statesToEnter, statesForDefaultEntry, defaultHistoryContent)
                    }
                }
            } else {
                if self.isParallelState(sid) {
                    for child in self.getChildStates(sid).iterator() {
                        if !statesToEnter.some(&|s| { self.isDescendant(*s, *child) }) {
                            self.addDescendantStatesToEnter(datamodel, *child, statesToEnter, statesForDefaultEntry, defaultHistoryContent)
                        }
                    }
                }
            }
        }
        self.tracer.traceResult("statesToEnter", &statesToEnter);
        self.tracer.exitMethod("addDescendantStatesToEnter");
    }

    /// #W3C says:
    /// # procedure addAncestorStatesToEnter(state, ancestor, statesToEnter, statesForDefaultEntry, defaultHistoryContent)
    /// Add to statesToEnter any ancestors of 'state' up to, but not including, 'ancestor' that must be entered in order to enter 'state'. If any of these ancestor states is a parallel state, we must fill in its descendants as well.
    /// ```
    /// procedure addAncestorStatesToEnter(state, ancestor, statesToEnter, statesForDefaultEntry, defaultHistoryContent)
    ///     for anc in getProperAncestors(state,ancestor):
    ///         statesToEnter.add(anc)
    ///         if isParallelState(anc):
    ///             for child in getChildStates(anc):
    ///                 if not statesToEnter.some(lambda s: isDescendant(s,child)):
    ///                     addDescendantStatesToEnter(child,statesToEnter,statesForDefaultEntry, defaultHistoryContent)
    /// ```
    fn addAncestorStatesToEnter(&self, datamodel: &mut dyn Datamodel, state: StateId, ancestor: StateId, statesToEnter: &mut OrderedSet<StateId>,
                                statesForDefaultEntry: &mut OrderedSet<StateId>, defaultHistoryContent: &mut HashTable<StateId, ExecutableContentId>) {
        self.tracer.enterMethod("addAncestorStatesToEnter");
        self.tracer.traceArgument("state", &state);
        for anc in self.getProperAncestors(state, ancestor).iterator() {
            statesToEnter.add(*anc);
            if self.isParallelState(*anc) {
                for child in self.getChildStates(*anc).iterator() {
                    if !statesToEnter.some(&|s| {
                        self.isDescendant(*s, *child)
                    }) {
                        self.addDescendantStatesToEnter(datamodel, *child, statesToEnter, statesForDefaultEntry, defaultHistoryContent);
                    }
                }
            }
        }
        self.tracer.exitMethod("addAncestorStatesToEnter");
    }

    /// #W3C says:
    /// # procedure isInFinalState(s)
    /// Return true if s is a compound \<state\> and one of its children is an active <final> state
    /// (i.e. is a member of the current configuration), or if s is a \<parallel\> state and
    /// isInFinalState is true of all its children.
    /// ```
    /// function isInFinalState(s):
    ///     if isCompoundState(s):
    ///         return getChildStates(s).some(lambda s: isFinalState(s) and configuration.isMember(s))
    ///     elif isParallelState(s):
    ///         return getChildStates(s).every(isInFinalState)
    ///     else:
    ///         return false
    /// ```
    fn isInFinalState(&self, datamodel: &dyn Datamodel, s: StateId) -> bool {
        if self.isCompoundState(s) {
            self.getChildStates(s).some(&|cs: &StateId| -> bool { self.isFinalStateId(*cs) && datamodel.global_s().configuration.isMember(cs) })
        } else if self.isParallelState(s) {
            self.getChildStates(s).every(&|cs: &StateId| -> bool { self.isInFinalState(datamodel, *cs) })
        } else {
            false
        }
    }


    /// #W3C says:
    /// # function getTransitionDomain(transition)
    /// Return the compound state such that
    /// 1) all states that are exited or entered as a result of taking 'transition'
    ///    are descendants of it
    /// 2) no descendant of it has this property.
    /// ```
    /// function getTransitionDomain(t)
    ///     tstates = getEffectiveTargetStates(t)
    ///     if not tstates:
    ///         return null
    ///     elif t.type == "internal" and isCompoundState(t.source) and tstates.every(lambda s: isDescendant(s,t.source)):
    ///         return t.source
    ///     else:
    ///         return findLCCA([t.source].append(tstates))
    /// ```
    fn getTransitionDomain(&self, datamodel: &mut dyn Datamodel, t: &Transition) -> StateId {
        self.tracer.enterMethod("getTransitionDomain");
        self.tracer.traceArgument("t", &t);
        let tstates = self.getEffectiveTargetStates(datamodel, t);
        let domain;
        if tstates.isEmpty() {
            domain = 0;
        } else if t.transition_type == TransitionType::Internal &&
            self.isCompoundState(t.source) &&
            tstates.every(&|s| -> bool { self.isDescendant(*s, t.source) })
        {
            domain = t.source;
        } else {
            let mut l = List::new();
            l.push(t.source);
            domain = self.findLCCA(&l.appendSet(&tstates));
        }
        self.tracer.traceResult("domain", &domain);
        self.tracer.exitMethod("getTransitionDomain");
        domain
    }

    /// #W3C says:
    /// # function findLCCA(stateList)
    /// The Least Common Compound Ancestor is the \<state\> or \<scxml\> element s such that s is a
    /// proper ancestor of all states on stateList and no descendant of s has this property.
    /// Note that there is guaranteed to be such an element since the <scxml> wrapper element is a
    /// common ancestor of all states. Note also that since we are speaking of proper ancestor
    /// (parent or parent of a parent, etc.) the LCCA is never a member of stateList.
    /// ```
    /// function findLCCA(stateList):
    ///     for anc in getProperAncestors(stateList.head(),null).filter(isCompoundStateOrScxmlElement):
    ///         if stateList.tail().every(lambda s: isDescendant(s,anc)):
    ///             return anc
    /// ```
    fn findLCCA(&self, stateList: &List<StateId>) -> StateId {
        self.tracer.enterMethod("findLCCA");
        self.tracer.traceArgument("stateList", stateList);
        let mut lcca = 0;
        for anc in self.getProperAncestors(*stateList.head(), 0)
            .toList().filterBy(&|s| { self.isCompoundStateOrScxmlElement(*s) }).iterator() {
            if stateList.tail().every(&|s| { self.isDescendant(*s, *anc) }) {
                lcca = *anc;
                break;
            }
        }
        self.tracer.traceResult("lcca", &lcca);
        self.tracer.exitMethod("findLCCA");
        lcca
    }

    /// #W3C says:
    /// # function getEffectiveTargetStates(transition)
    /// Returns the states that will be the target when 'transition' is taken, dereferencing any history states.
    /// ```
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
    ///     return targets
    /// ```
    fn getEffectiveTargetStates(&self, datamodel: &mut dyn Datamodel, transition: &Transition) -> OrderedSet<StateId> {
        self.tracer.enterMethod("getEffectiveTargetStates");
        self.tracer.traceArgument("transition", transition);
        let mut targets: OrderedSet<StateId> = OrderedSet::new();
        for sid in &transition.target {
            if self.isHistoryState(*sid) {
                if datamodel.global().historyValue.has(*sid) {
                    targets.union(datamodel.global().historyValue.get(*sid));
                } else {
                    let s = self.get_state_by_id(*sid);
                    // History states have exactly one "transition"
                    targets.union(&self.getEffectiveTargetStates(datamodel, self.get_transition_by_id(*s.transitions.head())));
                }
            } else {
                targets.add(*sid);
            }
        }
        self.tracer.traceResult("targets", &targets);
        self.tracer.exitMethod("getEffectiveTargetStates");
        targets
    }

    /// #W3C says:
    /// # function getProperAncestors(state1, state2)
    /// If state2 is null, returns the set of all ancestors of state1 in ancestry order
    /// (state1's parent followed by the parent's parent, etc. up to an including the <scxml> element).
    /// If state2 is non-null, returns in ancestry order the set of all ancestors of state1,
    /// up to but not including state2.
    /// (A "proper ancestor" of a state is its parent, or the parent's parent,
    /// or the parent's parent's parent, etc.))
    /// If state2 is state1's parent, or equal to state1, or a descendant of state1, this returns the empty set.
    fn getProperAncestors(&self, state1: StateId, state2: StateId) -> OrderedSet<StateId> {
        self.tracer.enterMethod("getProperAncestors");
        self.tracer.traceArgument("state1", &state1);
        self.tracer.traceArgument("state2", &state2);

        let mut properAncestors: OrderedSet<StateId> = OrderedSet::new();
        if !self.isDescendant(state2, state1) {
            let mut currState = self.get_state_by_id(state1).parent;
            while currState != 0 && currState != state2 {
                properAncestors.add(currState);
                currState = self.get_state_by_id(currState).parent;
            }
        }
        self.tracer.traceResult("properAncestors", &properAncestors);
        self.tracer.exitMethod("getProperAncestors");
        properAncestors
    }

    /// #W3C says:
    /// function isDescendant(state1, state2)
    /// Returns 'true' if state1 is a descendant of state2 (a child, or a child of a child, or a child of a child of a child, etc.) Otherwise returns 'false'.
    fn isDescendant(&self, state1: StateId, state2: StateId) -> bool {
        self.tracer.enterMethod("isDescendant");
        self.tracer.traceArgument("state1", &state1);
        self.tracer.traceArgument("state2", &state2);
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
        self.tracer.traceResult("result", &result);
        self.tracer.exitMethod("isDescendant");
        result
    }

    /// #W3C says:
    /// A Compound State: A state of type \<state\> with at least one child state.
    fn isCompoundState(&self, state: StateId) -> bool {
        if state != 0 {
            let stateS = self.get_state_by_id(state);
            !(stateS.is_final || stateS.is_parallel || stateS.states.is_empty())
        } else {
            false
        }
    }

    fn isCompoundStateOrScxmlElement(&self, sid: StateId) -> bool {
        sid == self.pseudo_root || !self.get_state_by_id(sid).states.is_empty()
    }

    fn isHistoryState(&self, state: StateId) -> bool {
        self.get_state_by_id(state).history_type != HistoryType::None
    }

    fn isCancelEvent(&self, ev: &Event) -> bool {
        // Cancel-Events (outer fsm cancels a fsm instance that was started by some invoke)
        // are platform specific.
        // TODO: we need a "invoke" concept!
        ev.name.ends_with(".cancel")
    }


    /// #W3C says:
    /// function getChildStates(state1)
    /// Returns a list containing all \<state\>, \<final\>, and \<parallel\> children of state1.
    fn getChildStates(&self, state1: StateId) -> List<StateId> {
        let mut l: List<StateId> = List::new();
        let stateRef = self.get_state_by_id(state1);
        for c in &stateRef.states {
            l.push(*c);
        }
        l
    }

    fn invoke(&mut self, _inv: &Invoke) {
        // We need a "invoke" concept!
        todo!()
    }

    fn cancelInvokeId(&mut self, _inv: InvokeId) {
        // TODO: we need a "invoke" concept!
        // Send a cancel event to the thread/process.
        // see isCancelEvent
    }

    fn cancelInvoke(&mut self, _inv: &Invoke) {
        // TODO: we need a "invoke" concept!
        // Send a cancel event to the thread/pricess.
        // see isCancelEvent
    }


    fn applyFinalize(&mut self, _invokeId: InvokeId, _event: &Event) {
        todo!()
    }

    fn send(&mut self, _invokeId: InvokeId, _event: &Event) {
        todo!()
    }


    /// #W3C says:
    /// 5.9.1 Conditional Expressions
    /// Conditional expressions are used inside the 'cond' attribute of \<transition\>, \<if\> and \<elseif\>.
    /// If a conditional expression cannot be evaluated as a boolean value ('true' or 'false') or if
    /// its evaluation causes an error, the SCXML Processor must treat the expression as if it evaluated to
    /// 'false' and must place the error 'error.execution' in the internal event queue.
    ///
    /// See [Datamodel::executeCondition]
    fn conditionMatch(&mut self, datamodel: &mut dyn Datamodel, tid: TransitionId) -> bool
    {
        let cond;
        {
            let t = self.get_transition_by_id_mut(tid);
            cond = t.cond.clone();
        }
        match cond {
            Some(c) => {
                match datamodel.executeCondition(self, &c) {
                    Ok(v) => v,
                    Err(_e) => {
                        self.enqueue_internal(datamodel, Event::error("execution"));
                        false
                    }
                }
            }
            None => {
                true
            }
        }
    }

    fn nameMatch(&self, events: &Vec<String>, name: &String) -> bool
    {
        events.contains(name)
    }

    /// Converts a set of ids to list of references.
    fn set_to_state_list(&self, stateIds: &OrderedSet<StateId>) -> List<&State> {
        let mut l = List::new();
        for sid in stateIds.iterator() {
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
    fn to_transition_list(&self, transIds: &List<TransitionId>) -> List<&Transition> {
        let mut l = List::new();
        for tid in transIds.iterator() {
            l.push(self.get_transition_by_id(*tid));
        }
        l
    }

    pub fn schedule<F>(&self, delay_ms: i64, cb: F)
        where F: 'static + FnMut() + Send {
        self.timer.schedule_with_delay(chrono::Duration::milliseconds(delay_ms), cb).ignore();
    }
}

#[derive(Debug)]
pub struct SrcData {
    pub src: String,
    pub content: Option<String>,
}

impl SrcData {
    pub fn new() -> SrcData {
        SrcData { src: "".to_string(), content: None }
    }
}

impl ToString for SrcData {
    fn to_string(&self) -> String {
        self.src.clone()
    }
}

impl Data for SrcData {
    fn get_copy(&self) -> Box<dyn Data> {
        Box::new(SrcData {
            src: self.src.clone(),
            content: self.content.clone(),
        })
    }
}

#[derive(Debug)]
pub struct ExpressionData {
    pub expr: String,
}

impl ExpressionData {
    pub fn new() -> ExpressionData {
        ExpressionData { expr: "".to_string() }
    }
}

impl ToString for ExpressionData {
    fn to_string(&self) -> String {
        self.expr.clone()
    }
}

impl Data for ExpressionData {
    fn get_copy(&self) -> Box<dyn Data> {
        Box::new(ExpressionData {
            expr: self.expr.clone(),
        })
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

    pub fn get(&self, key: &String) -> Option<&Box<dyn Data>> {
        if self.values.contains_key(key) {
            self.values.get(key)
        } else {
            None
        }
    }

    pub fn set(&mut self, key: &String, data: Box<dyn Data>) {
        self.values.insert(key.clone(), data);
    }
}

#[derive(Clone)]
#[derive(Debug)]
pub struct DoneData {
    /// content of \<content\> child
    pub content: String,
    /// expr-attribute of \<content\> child
    pub content_expr: String,
}

impl DoneData {
    pub fn new() -> DoneData {
        DoneData {
            content: String::new(),
            content_expr: String::new(),
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
/// |id|false|none|ID|none|A valid id as defined in [https://www.w3.org/TR/scxml/#Schema](XML Schema)|The identifier for this state. See 3.14 IDs for details.|
/// |initial|false|MUST NOT be specified in conjunction with the \<initial\> element. MUST NOT occur in atomic states.|IDREFS|none|A legal state specification. See 3.11 Legal State Configurations and Specifications for details.|The id of the default initial state (or states) for this state.|
///
/// 3.3.2 Children
/// - \<onentry\> Optional element holding executable content to be run upon entering this <state>.
///   Occurs 0 or more times. See 3.8 \<onentry\>
/// - \<onexit\> Optional element holding executable content to be run when exiting this <state>.
///   Occurs 0 or more times. See 3.9 \<onexit\>
/// - \<transition\> Defines an outgoing transition from this state. Occurs 0 or more times.
///   See 3.5 <transition>
/// - \<initial\> In states that have substates, an optional child which identifies the default
///   initial state. Any transition which takes the parent state as its target will result in the
///   state machine also taking the transition contained inside the <initial> element.
///   See 3.6 \<initial\>
/// - \<state\> Defines a sequential substate of the parent state. Occurs 0 or more times.
/// - \<parallel\> Defines a parallel substate. Occurs 0 or more times. See 3.4 \<parallel\>
/// - \<final\>. Defines a final substate. Occurs 0 or more times. See 3.7 \<final\>.
/// - \<history\> A child pseudo-state which records the descendant state(s) that the parent state
///   was in the last time the system transitioned from the parent.
///   May occur 0 or more times. See 3.10 \<history\>.
/// - \<datamodel\> Defines part or all of the data model. Occurs 0 or 1 times. See 5.2 \<datamodel\>
/// - \<invoke> Invokes an external service. Occurs 0 or more times. See 6.4 \<invoke\> for details.
///
/// ##Definitions:
/// - An atomic state is a \<state\> that has no \<state\>, \<parallel\> or \<final\> children.
/// - A compound state is a \<state\> that has \<state\>, \<parallel\>, or \<final\> children
///   (or a combination of these).
/// - The default initial state(s) of a compound state are those specified by the 'initial' attribute
///   or \<initial\> element, if either is present. Otherwise it is the state's first child state
///   in document order.
///
/// In a conformant SCXML document, a compound state may specify either an "initial" attribute or an
/// \<initial\> element, but not both. See 3.6 \<initial\> for a discussion of the difference between
/// the two notations.
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
    pub onentry: ExecutableContentId,

    /// The script that is executed if the state is left. See W3c comments for \<onexit\> above.
    pub onexit: ExecutableContentId,

    /// All transitions between sub-states.
    pub transitions: List<TransitionId>,

    pub invoke: List<Invoke>,
    pub history: List<StateId>,

    /// The local datamodel
    pub data: DataStore,

    pub isFirstEntry: bool,
    pub parent: StateId,
    pub donedata: Option<DoneData>,
}

impl State {
    pub fn new(name: &String) -> State {
        State {
            id: 0,
            doc_id: 0,
            name: name.clone(),
            initial: 0,
            states: vec![],
            onentry: 0,
            onexit: 0,
            transitions: List::new(),
            is_parallel: false,
            is_final: false,
            history_type: HistoryType::None,
            /// True if the state was never entered before.
            data: DataStore::new(),
            isFirstEntry: false,
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

#[derive(Debug)]
#[derive(PartialEq)]
#[derive(Clone)]
pub enum HistoryType {
    Shallow,
    Deep,
    None,
}

pub fn map_history_type(ts: &String) -> HistoryType {
    match ts.to_lowercase().as_str() {
        "deep" => HistoryType::Deep,
        "shallow" => HistoryType::Shallow,
        "" => HistoryType::None,
        _ => panic!("Unknown transition type '{}'", ts),
    }
}

#[derive(Debug)]
#[derive(PartialEq)]
pub enum TransitionType {
    Internal,
    External,
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

pub type TransitionId = u32;

#[derive(Debug)]
pub struct Transition {
    pub id: TransitionId,
    pub doc_id: DocumentId,

    // TODO: Possibly we need some type to express event ids
    pub events: Vec<String>,
    pub cond: Option<String>,
    pub source: StateId,
    pub target: Vec<StateId>,
    pub transition_type: TransitionType,
    pub content: ExecutableContentId,
}

impl PartialEq for Transition {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }

    fn ne(&self, other: &Self) -> bool {
        self.id != other.id
    }
}

impl Transition {
    pub fn new() -> Transition {
        let idc = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        Transition {
            id: idc,
            doc_id: 0,
            events: vec![],
            cond: None,
            source: 0,
            target: vec![],
            transition_type: TransitionType::Internal,
            content: 0,
        }
    }
}

pub trait Data: Send + Debug + ToString {
    fn get_copy(&self) -> Box<dyn Data>;
}

pub struct SimpleData {
    pub value: String,
}

impl Debug for SimpleData {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl ToString for SimpleData {
    fn to_string(&self) -> String {
        self.value.clone()
    }
}

impl Data for SimpleData {
    fn get_copy(&self) -> Box<dyn Data> {
        Box::new(SimpleData {
            value: self.value.clone(),
        })
    }
}

/// Datamodel interface trait.
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
    /// As the datamodel needs access to other global variables and rust doesn't like
    /// accessing data of parents (Fsm in this case) from inside a member (the actual Datmodel), most global data is
    /// store in the "GlobalData" struct that is owned by the datamodel.    ///
    fn global(&mut self) -> &mut GlobalData;

    fn global_s(&self) -> &GlobalData;

    /// Get the name of the datamodel as defined by the \<scxml\> attribute "datamodel".
    fn get_name(self: &Self) -> &str;

    /// Initialize the datamodel for one data-store.
    /// This method is called for the global data and for the data of each state.
    fn initializeDataModel(&mut self, fsm: &mut Fsm, state: StateId);

    /// Sets a global variable.
    fn set(&mut self, name: &String, data: Box<dyn Data>);

    /// Gets a global variable.
    fn get(&self, name: &String) -> Option<&dyn Data>;

    /// Clear all.
    fn clear(&mut self);

    /// "log" function, use for \<log\> content.
    fn log(&mut self, msg: &String);

    /// Execute a script.
    fn execute(&mut self, fsm: &Fsm, script: &String) -> String;

    fn executeForEach(&mut self, fsm: &Fsm, array_expression: &String, item: &String, index: &String,
                      execute_body: &mut dyn FnMut(&mut dyn Datamodel));

    /// #W3C says:
    /// The set of operators in conditional expressions varies depending on the data model,
    /// but all data models must support the 'In()' predicate, which takes a state ID as its
    /// argument and returns true if the state machine is in that state.\
    /// Conditional expressions in conformant SCXML documents should not have side effects.
    /// #Actual Implementation:
    /// As no side-effects shall occur, this method should be "&self". But we assume that most script-engines have
    /// no read-only "eval" function and such method may be hard to implement.
    fn executeCondition(&mut self, fsm: &Fsm, script: &String) -> Result<bool, String>;

    fn executeContent(&mut self, fsm: &Fsm, contentId: ExecutableContentId);
}

pub fn createDatamodel(name: &str) -> Box<dyn Datamodel> {
    match name.to_lowercase().as_str() {
        // TODO: use some registration api to handle data-models
        #[cfg(feature = "ECMAScript")]
        ECMA_SCRIPT_LC => Box::new(ECMAScriptDatamodel::new()),
        NULL_DATAMODEL_LC => Box::new(NullDatamodel::new()),
        _ => panic!("Unsupported Datamodel '{}'", name)
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
    pub global: GlobalData,
}

impl NullDatamodel {
    pub fn new() -> NullDatamodel {
        NullDatamodel {
            global: GlobalData::new(),
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
    fn initializeDataModel(self: &mut Self, _fsm: &mut Fsm, _dataState: StateId) {}

    fn set(self: &mut NullDatamodel, _name: &String, _data: Box<dyn Data>) {
        // nothing to do
    }

    fn get(self: &NullDatamodel, _name: &String) -> Option<&dyn Data> {
        None
    }

    fn clear(self: &mut NullDatamodel) {}

    fn log(self: &mut NullDatamodel, msg: &String) {
        println!("Log: {}", msg);
    }

    fn execute(&mut self, _fsm: &Fsm, _script: &String) -> String {
        "".to_string()
    }

    fn executeForEach(&mut self, _fsm: &Fsm, _array_expression: &String, _item: &String, _index: &String,
                      _executeBody: &mut dyn FnMut(&mut dyn Datamodel)) {
        // nothing to do
    }

    /// #W3C says:
    /// The boolean expression language consists of the In predicate only.
    /// It has the form 'In(id)', where id is the id of a state in the enclosing state machine.
    /// The predicate must return 'true' if and only if that state is in the current state configuration.
    fn executeCondition(&mut self, _fsm: &Fsm, _script: &String) -> Result<bool, String> {
        // TODO: Support for "In" predicate
        Result::Ok(false)
    }

    fn executeContent(&mut self, _fsm: &Fsm, _content_id: ExecutableContentId) {
        // Nothing
    }
}


////////////////////////////////////////
//// Display support

impl Display for Fsm {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Fsm{{v:{} root:{} states:", self.version, self.pseudo_root)?;
        display_state_map(&self.states, f)?;
        display_transition_map(&self.transitions, f)?;
        write!(f, "}}")
    }
}

impl Display for State {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{{#{} states:{} transitions: {}}}", self.id, vecToString(&self.states),
               vecToString(&self.transitions.data)
        )
    }
}

impl Display for Transition {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{{#{} {} {:?} target:{:?}}}",
               self.id,
               self.transition_type, &self.events,
               self.target)
    }
}

impl Display for TransitionType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TransitionType::Internal => f.write_str("internal"),
            TransitionType::External => f.write_str("external")
        }
    }
}

impl Display for List<u32> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(vecToString(&self.data).as_str())
    }
}

impl Display for List<&State> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(vecToString(&self.data).as_str())
    }
}

impl Display for OrderedSet<u32> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(vecToString(&self.data).as_str())
    }
}

pub(crate) fn vecToString<T: Display>(v: &Vec<T>) -> String {
    let mut s = "[".to_string();

    let len = v.len();
    for i in 0..len {
        s += format!("{}{}",
                     if i > 0 {
                         ","
                     } else {
                         ""
                     }, v[i]).as_str();
    }
    s += "]";
    s
}

#[cfg(test)]
mod tests {
    use std::{thread, time};
    use std::sync::mpsc::Sender;

    use crate::{Event, EventType, fsm, reader, Trace};
    use crate::fsm::List;
    use crate::fsm::OrderedSet;

    fn test_send(sender: &Sender<Box<Event>>, e: Event)
    {
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

        let m = l.some(&|s| -> bool {
            *s == "Abc".to_string()
        });

        assert_eq!(m, true);
    }

    #[test]
    fn list_can_every() {
        let mut l: List<String> = List::new();
        l.push("Abc".to_string());
        l.push("def".to_string());
        l.push("ghi".to_string());
        l.push("xyz".to_string());

        let mut m = l.every(&|_s| -> bool {
            true
        });
        assert_eq!(m, true);

        m = l.every(&|s| -> bool {
            !s.eq(&"ghi".to_string())
        });
        assert_eq!(m, false);
    }

    #[test]
    fn list_can_filter() {
        let mut l: List<String> = List::new();
        l.push("Abc".to_string());
        l.push("def".to_string());
        l.push("ghi".to_string());
        l.push("xyz".to_string());

        let l2: List<String> = l.filterBy(&|_s: &String| -> bool {
            true
        });
        assert_eq!(l2.size(), l.size());

        let l3 = l2.filterBy(&|_s: &String| -> bool {
            false
        });
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
        let mut l1V: Vec<String> = Vec::new();

        let mut l2 = l1.sort(&|a, b| a.partial_cmp(b).unwrap());

        while l1.size() > 0 {
            let e = l1.head();
            println!(" {}", e);
            l1V.push(e.clone());
            l1 = l1.tail();
        }
        l1V.sort_by(&|a: &String, b: &String| a.partial_cmp(b).unwrap());

        assert_eq!(l1V.len(), l2.size());

        println!("Sorted ======");
        let mut i = 0;
        while l2.size() > 0 {
            let h = l2.head().clone();
            l2 = l2.tail();
            println!(" {}", h);
            assert_eq!(h.eq(l1V.get(i).unwrap()), true);
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
        assert_eq!(os1.isMember(&"def2".to_string()), true);
        assert_eq!(os1.isMember(&"Abc".to_string()), true);
    }

    #[test]
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

        let m = os.some(&|s| -> bool {
            *s == "Abc".to_string()
        });

        assert_eq!(m, true);
    }

    #[test]
    fn ordered_set_can_every() {
        let mut os: OrderedSet<String> = OrderedSet::new();
        os.add("Abc".to_string());
        os.add("def".to_string());
        os.add("ghi".to_string());
        os.add("xyz".to_string());

        let mut m = os.every(&|_s| -> bool {
            true
        });
        assert_eq!(m, true);

        m = os.every(&|s| -> bool {
            !s.eq(&"ghi".to_string())
        });
        assert_eq!(m, false);
    }

    #[test]
    fn ordered_set_can_hasIntersection() {
        let mut os1: OrderedSet<String> = OrderedSet::new();
        os1.add("Abc".to_string());
        os1.add("def".to_string());
        os1.add("ghi".to_string());
        os1.add("xyz".to_string());

        let mut os2: OrderedSet<String> = OrderedSet::new();

        let mut m = os1.hasIntersection(&os2);
        assert_eq!(m, false);

        // One common elements
        os2.add("Abc".to_string());
        m = os1.hasIntersection(&os2);
        assert_eq!(m, true);

        // Same other un-common elements
        os2.add("Def".to_string());
        os2.add("Ghi".to_string());
        os2.add("Xyz".to_string());
        m = os1.hasIntersection(&os2);
        assert_eq!(m, true);

        // Same with TWO common elements
        os2.add("def".to_string());
        m = os1.hasIntersection(&os2);
        assert_eq!(m, true);

        // Remove common elements from first
        os1.delete(&"Abc".to_string());
        os1.delete(&"def".to_string());
        m = os1.hasIntersection(&os2);
        assert_eq!(m, false);

        // Always common with itself
        m = os1.hasIntersection(&os1);
        assert_eq!(m, true);

        // but not if empty
        os1.clear();
        m = os1.hasIntersection(&os1);
        // Shall return false
        assert_eq!(m, false);
    }

    #[test]
    fn ordered_set_can_isEmpty() {
        let mut os1: OrderedSet<String> = OrderedSet::new();
        assert_eq!(os1.isEmpty(), true);

        os1.add("Abc".to_string());
        assert_eq!(os1.isEmpty(), false);
    }

    #[test]
    fn ordered_set_can_clear() {
        let mut os1: OrderedSet<String> = OrderedSet::new();
        os1.add("Abc".to_string());
        os1.clear();
        assert_eq!(os1.isEmpty(), true);
    }


    #[test]
    fn fsm_shall_exit() {
        println!("Creating The SM:");
        let sm = reader::read_from_xml(
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
    </scxml>".to_string());

        assert!(!sm.is_err(), "FSM shall be parsed");

        let mut fsm = sm.unwrap();
        fsm.tracer.enableTrace(Trace::ALL);

        let (threadHandle, sender) = fsm::start_fsm(fsm);

        let t_millis = time::Duration::from_millis(1000);
        thread::sleep(t_millis);

        println!("Send Event");

        let emptyStr = "".to_string();

        test_send(&sender, Event { name: "ab".to_string(), etype: EventType::platform, sendid: 0, origin: emptyStr.clone(), origintype: emptyStr.clone(), invokeid: 1, data: None });
        test_send(&sender, Event { name: "exit".to_string(), etype: EventType::platform, sendid: 0, origin: emptyStr.clone(), origintype: emptyStr.clone(), invokeid: 2, data: None });

        // TODO: How to check for timeouts??
        let _r = threadHandle.join();
    }
}
