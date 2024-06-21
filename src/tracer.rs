use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt::{Debug, Display, Formatter};
use std::fmt;
use std::ops::DerefMut;
use std::str::FromStr;

use log::info;

use crate::{ArgOption, fsm};
use crate::fsm::{Event, OrderedSet, State};

/// Trace mode for FSM Tracer.
#[derive(Debug, Clone, PartialEq, Copy, Hash, Eq)]
pub enum TraceMode {
    METHODS,
    STATES,
    EVENTS,
    ARGUMENTS,
    RESULTS,
    ALL,
}

pub static TRACE_ARGUMENT_OPTION: ArgOption =
    ArgOption { name: "trace", with_value: true, required: false };

impl TraceMode {
    /// Parse Trace-mode from program arguments.
    pub fn from_arguments(named_arguments: &HashMap::<&'static str, String>) -> TraceMode {
        let mut trace = TraceMode::STATES;

        match named_arguments.get("trace") {
            None => {}
            Some(trace_name) => {
                match TraceMode::from_str(trace_name) {
                    Ok(opt) => {
                        trace = opt;
                    }
                    Err(_err) => {
                        panic!("Unknown trace mode '{}'", trace_name)
                    }
                }
            }
        }
        trace
    }
}

impl Display for TraceMode {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        Debug::fmt(self, f)
    }
}

impl FromStr for TraceMode {
    type Err = ();

    fn from_str(input: &str) -> Result<TraceMode, Self::Err> {
        match input.to_lowercase().as_str() {
            "methods" => Ok(TraceMode::METHODS),
            "states" => Ok(TraceMode::STATES),
            "events" => Ok(TraceMode::EVENTS),
            "arguments" => Ok(TraceMode::ARGUMENTS),
            "results" => Ok(TraceMode::RESULTS),
            "all" => Ok(TraceMode::ALL),
            _ => Err(()),
        }
    }
}

/// Trait used to trace methods and
/// states inside the FSM. What is traced can be controlled by
/// [Tracer::enable_trace] and [Tracer::disable_trace], see [TraceMode].
pub trait Tracer: Send + Debug {
    fn trace(&self, msg: &str);

    /// Enter a sub-scope, e.g. by increase the log indentation.
    fn enter(&self);

    /// Leave the current sub-scope, e.g. by decrease the log indentation.
    fn leave(&self);

    /// Enable traces for the specified scope.
    fn enable_trace(&mut self, flag: TraceMode);

    /// Disable traces for the specified scope.
    fn disable_trace(&mut self, flag: TraceMode);

    /// Return true if the given scape is enabled.
    fn is_trace(&self, flag: TraceMode) -> bool;

    /// Called by FSM if a method is entered
    fn enter_method(&self, what: &str) {
        if self.is_trace(TraceMode::METHODS) {
            self.trace(format!(">>> {}", what).as_str());
            self.enter();
        }
    }

    /// Called by FSM if a method is exited
    fn exit_method(&self, what: &str) {
        if self.is_trace(TraceMode::METHODS) {
            self.leave();
            self.trace(format!("<<< {}", what).as_str());
        }
    }

    /// Called by FSM if an internal event is send
    fn event_internal_send(&self, what: &Event) {
        if self.is_trace(TraceMode::EVENTS) {
            self.trace(format!("Int<- {} #{}", what.name, what.invoke_id).as_str());
        }
    }

    /// Called by FSM if an internal event is received
    fn event_internal_received(&self, what: &Event) {
        if self.is_trace(TraceMode::EVENTS) {
            self.trace(format!("Internal Event-> {}, invokeId {}", what.name, what.invoke_id).as_str());
        }
    }

    /// Called by FSM if an external event is send
    fn event_external_send(&self, what: &Event) {
        if self.is_trace(TraceMode::EVENTS) {
            self.trace(format!("Ext<- {} #{}", what.name, what.invoke_id).as_str());
        }
    }

    /// Called by FSM if an external event is received
    fn event_external_received(&mut self, what: &Event) {
        if what.name.starts_with("trace.") {
            let p = what.name.as_str().split('.').collect::<Vec<&str>>();
            if p.len() == 3 {
                match TraceMode::from_str(p.get(1).unwrap()) {
                    Ok(t) => {
                        match *p.get(2).unwrap() {
                            "on" | "ON" | "On" => {
                                self.enable_trace(t);
                            }
                            "off" | "OFF" | "Off" => {
                                self.disable_trace(t);
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
        if self.is_trace(TraceMode::EVENTS) {
            self.trace(format!("Ext-> {} #{}", what.name, what.invoke_id).as_str());
        }
    }


    /// Called by FSM if a state is entered or left.
    fn trace_state(&self, what: &str, s: &State) {
        if self.is_trace(TraceMode::STATES) {
            if s.name.is_empty() {
                self.trace(format!("{} #{}", what, s.id).as_str());
            } else {
                self.trace(format!("{} <{}> #{}", what, &s.name, s.id).as_str());
            }
        }
    }

    /// Called by FSM if a state is entered. Calls [traceState].
    fn trace_enter_state(&self, s: &State) {
        self.trace_state("Enter", s);
    }

    /// Called by FSM if a state is left. Calls [traceState].
    fn trace_exit_state(&self, s: &State) {
        self.trace_state("Exit", s);
    }


    /// Called by FSM for input arguments in methods.
    fn trace_argument(&self, what: &str, d: &dyn Display) {
        if self.is_trace(TraceMode::ARGUMENTS) {
            self.trace(format!("In:{}:{}", what, d).as_str());
        }
    }

    /// Called by FSM for results in methods.
    fn trace_result(&self, what: &str, d: &dyn Display) {
        if self.is_trace(TraceMode::RESULTS) {
            self.trace(format!("Out:{}:{}", what, d).as_str());
        }
    }

    /// Helper method to trace a vector of ids.
    fn trace_id_vec(&self, what: &str, l: &Vec<u32>) {
        self.trace(format!("{}: {}", what, fsm::vec_to_string(&l)).as_str());
    }

    /// Helper method to trace a OrderedSet of ids.
    fn trace_id_set(&self, what: &str, l: &OrderedSet<u32>) {
        self.trace(format!("{}: {}", what, fsm::vec_to_string(&l.data)).as_str());
    }
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

    fn enable_trace(&mut self, flag: TraceMode) {
        self.trace_flags.insert(flag);
    }

    fn disable_trace(&mut self, flag: TraceMode) {
        self.trace_flags.remove(&flag);
    }

    fn is_trace(&self, flag: TraceMode) -> bool {
        self.trace_flags.contains(&flag) || self.trace_flags.contains(&TraceMode::ALL)
    }
}

#[derive(Debug)]
pub struct DefaultTracer {
    pub trace_flags: HashSet<TraceMode>,
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


thread_local! {
    /// Trace prefix for [DefaultTracer]
    static TRACE_PREFIX: RefCell<String> = RefCell::new("".to_string());
 }
