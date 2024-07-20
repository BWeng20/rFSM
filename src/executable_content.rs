//! Implementation of "executable content" elements.\
//! See [W3C:Executable Content](/doc/W3C_SCXML_2024_07_13/index.html#executable).

use std::collections::HashMap;
use std::fmt;
use std::fmt::{Debug, Display, Formatter};
#[cfg(test)]
use std::{println as info, println as warn};

use lazy_static::lazy_static;
use log::error;
#[cfg(not(test))]
use log::{info, warn};
use regex::Regex;

use crate::datamodel::{Datamodel, ToAny, SCXML_EVENT_PROCESSOR};
use crate::fsm::opt_vec_to_string;
#[cfg(feature = "BasicHttpEventIOProcessor")]
use crate::fsm::{vec_to_string, Cancel, ExecutableContentId, Fsm, Parameter, SendParameters};
use crate::scxml_event_io_processor::SCXML_TARGET_INTERNAL;
use crate::{get_global, Event, EventType};

pub const TARGET_SCXML_EVENT_PROCESSOR: &str = "http://www.w3.org/TR/scxml/#SCXMLEventProcessor";

pub const TYPE_IF: &str = "if";
pub const TYPE_EXPRESSION: &str = "expression";
pub const TYPE_SCRIPT: &str = "script";
pub const TYPE_LOG: &str = "log";
pub const TYPE_FOREACH: &str = "foreach";
pub const TYPE_SEND: &str = "send";
pub const TYPE_RAISE: &str = "raise";
pub const TYPE_CANCEL: &str = "cancel";
pub const TYPE_ASSIGN: &str = "assign";

pub trait ExecutableContent: ToAny + Debug + Send {
    fn execute(&self, datamodel: &mut dyn Datamodel, fsm: &Fsm);
    fn get_type(&self) -> &str;
    fn trace(&self, tracer: &mut dyn ExecutableContentTracer, fsm: &Fsm);
}

pub fn get_safe_executable_content_as<T: 'static>(ec: &mut dyn ExecutableContent) -> &mut T {
    let va = ec.as_any();
    va.downcast_mut::<T>()
        .unwrap_or_else(|| panic!("Failed to cast executable content"))
}

pub fn get_executable_content_as<T: 'static>(ec: &mut dyn ExecutableContent) -> Option<&mut T> {
    let va = ec.as_any();
    match va.downcast_mut::<T>() {
        Some(v) => Some(v),
        None => None,
    }
}

pub fn get_opt_executable_content_as<T: 'static>(
    ec_opt: Option<&mut dyn ExecutableContent>,
) -> Option<&mut T> {
    match ec_opt {
        Some(ec) => get_executable_content_as::<T>(ec),
        None => None,
    }
}

pub trait ExecutableContentTracer {
    fn print_name_and_attributes(&mut self, ec: &dyn ExecutableContent, attrs: &[(&str, &String)]);
    fn print_sub_content(&mut self, name: &str, fsm: &Fsm, content: ExecutableContentId);
}

#[derive(Debug)]
pub struct Script {
    pub content: Vec<ExecutableContentId>,
}

#[derive(Debug)]
pub struct Expression {
    pub content: String,
}

#[derive(Debug)]
pub struct Log {
    pub label: String,
    pub expression: String,
}

#[derive(Debug)]
pub struct If {
    pub condition: String,
    pub content: ExecutableContentId,
    pub else_content: ExecutableContentId,
}

#[derive(Debug)]
pub struct ForEach {
    pub array: String,
    pub item: String,
    pub index: String,
    pub content: ExecutableContentId,
}

/// *W3C says*:
/// The \<raise\> element raises an event in the current SCXML session.\
/// Note that the event will not be processed until the current block of executable content has completed
/// and all events that are already in the internal event queue have been processed. For example, suppose
/// the \<raise\> element occurs first in the \<onentry\> handler of state S followed by executable content
/// elements ec1 and ec2. If event e1 is already in the internal event queue when S is entered, the event
/// generated by \<raise\> will not be processed until ec1 and ec2 have finished execution and e1 has been
/// processed.
///
pub struct Raise {
    pub event: String,
}

pub struct Assign {
    pub location: String,
    pub expr: String,
}

impl Assign {
    pub fn new() -> Assign {
        Assign {
            location: String::new(),
            expr: String::new(),
        }
    }
}

impl Debug for Assign {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Assign")
            .field("location", &self.location)
            .field("expr", &self.expr)
            .finish()
    }
}

impl ExecutableContent for Assign {
    fn execute(&self, datamodel: &mut dyn Datamodel, _fsm: &Fsm) {
        datamodel.assign(&self.location.as_str(), &self.expr);
    }

    fn get_type(&self) -> &str {
        TYPE_ASSIGN
    }

    fn trace(&self, tracer: &mut dyn ExecutableContentTracer, _fsm: &Fsm) {
        tracer
            .print_name_and_attributes(self, &[("location", &self.location), ("expr", &self.expr)]);
    }
}

impl Raise {
    pub fn new() -> Raise {
        Raise {
            event: String::new(),
        }
    }
}

impl Debug for Raise {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Raise").field("event", &self.event).finish()
    }
}

impl ExecutableContent for Raise {
    fn execute(&self, datamodel: &mut dyn Datamodel, _fsm: &Fsm) {
        let event = Event::new("", &self.event, None, None);
        get_global!(datamodel).enqueue_internal(event);
    }

    fn get_type(&self) -> &str {
        TYPE_RAISE
    }

    fn trace(&self, tracer: &mut dyn ExecutableContentTracer, _fsm: &Fsm) {
        tracer.print_name_and_attributes(self, &[("event", &self.event)]);
    }
}

impl Script {
    pub fn new() -> Script {
        Script {
            content: Vec::new(),
        }
    }
}

impl ExecutableContent for Script {
    fn execute(&self, datamodel: &mut dyn Datamodel, fsm: &Fsm) {
        for s in &self.content {
            let _l = datamodel.executeContent(fsm, *s);
        }
    }

    fn get_type(&self) -> &str {
        TYPE_SCRIPT
    }

    fn trace(&self, tracer: &mut dyn ExecutableContentTracer, _fsm: &Fsm) {
        // TODO: Shall we print any sub-content?
        tracer.print_name_and_attributes(self, &[("content", &vec_to_string(&self.content))]);
    }
}

impl Expression {
    pub fn new() -> Expression {
        Expression {
            content: String::new(),
        }
    }
}

impl ExecutableContent for Expression {
    fn execute(&self, datamodel: &mut dyn Datamodel, _fsm: &Fsm) {
        let _l = datamodel.execute(&self.content);
    }

    fn get_type(&self) -> &str {
        TYPE_EXPRESSION
    }

    fn trace(&self, tracer: &mut dyn ExecutableContentTracer, _fsm: &Fsm) {
        tracer.print_name_and_attributes(self, &[("content", &self.content)]);
    }
}

impl Log {
    pub fn new(label: &Option<&String>, expression: &str) -> Log {
        Log {
            label: label.unwrap_or(&"".to_string()).clone(),
            expression: expression.to_string(),
        }
    }
}

impl ExecutableContent for Log {
    fn execute(&self, datamodel: &mut dyn Datamodel, _fsm: &Fsm) {
        let l = datamodel.execute(&self.expression);
        if l.is_some() {
            datamodel.log(&l.unwrap());
        }
    }

    fn get_type(&self) -> &str {
        TYPE_LOG
    }

    fn trace(&self, tracer: &mut dyn ExecutableContentTracer, _fsm: &Fsm) {
        tracer.print_name_and_attributes(self, &[("expression", &self.expression)]);
    }
}

impl If {
    pub fn new(condition: &String) -> If {
        If {
            condition: condition.clone(),
            content: 0,
            else_content: 0,
        }
    }
}

impl ExecutableContent for If {
    fn execute(&self, datamodel: &mut dyn Datamodel, fsm: &Fsm) {
        let r = datamodel.execute_condition(&self.condition).unwrap_or_else(|e| {
            warn!("Condition {} can't be evaluated. {}", self.condition, e);
            false
        });
        if r {
            if self.content != 0 {
                for e in fsm.executableContent.get(&self.content).unwrap() {
                    e.execute(datamodel, fsm);
                }
            }
        } else {
            if self.else_content != 0 {
                for e in fsm.executableContent.get(&self.else_content).unwrap() {
                    e.execute(datamodel, fsm);
                }
            }
        }
    }

    fn get_type(&self) -> &str {
        TYPE_IF
    }

    fn trace(&self, tracer: &mut dyn ExecutableContentTracer, fsm: &Fsm) {
        tracer.print_name_and_attributes(self, &[("condition", &self.condition)]);
        tracer.print_sub_content("then", fsm, self.content);
        tracer.print_sub_content("else", fsm, self.else_content);
    }
}

pub const INDEX_TEMP: &str = "__$index";

impl ForEach {
    pub fn new() -> ForEach {
        ForEach {
            array: "".to_string(),
            item: "".to_string(),
            index: "".to_string(),
            content: 0,
        }
    }
}

impl ExecutableContent for ForEach {
    fn execute(&self, datamodel: &mut dyn Datamodel, fsm: &Fsm) {
        let idx = if self.index.is_empty() {
            INDEX_TEMP.to_string()
        } else {
            self.index.clone()
        };
        datamodel.execute_for_each(&self.array, &self.item, &idx, &mut |datamodel| {
            if self.content != 0 {
                for e in fsm.executableContent.get(&self.content).unwrap() {
                    e.execute(datamodel, fsm);
                }
            }
        });
    }

    fn get_type(&self) -> &str {
        TYPE_FOREACH
    }

    fn trace(&self, tracer: &mut dyn ExecutableContentTracer, fsm: &Fsm) {
        tracer.print_name_and_attributes(
            self,
            &[
                ("array", &self.array),
                ("item", &self.item),
                ("index", &self.index),
            ],
        );
        tracer.print_sub_content("content", fsm, self.content);
    }
}

impl Parameter {
    pub fn new() -> Parameter {
        Parameter {
            name: "".to_string(),
            expr: "".to_string(),
            location: "".to_string(),
        }
    }
}

impl Display for Parameter {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Parameter{{name:{} expr:{} location:{}}}",
            self.name, self.expr, self.location
        )
    }
}

impl ExecutableContent for Cancel {
    fn execute(&self, _datamodel: &mut dyn Datamodel, _fsm: &Fsm) {
        todo!()
    }

    fn get_type(&self) -> &str {
        TYPE_CANCEL
    }

    fn trace(&self, tracer: &mut dyn ExecutableContentTracer, _fsm: &Fsm) {
        tracer.print_name_and_attributes(
            self,
            &[
                ("sendid", &self.send_id),
                ("sendidexpr", &self.send_id_expr),
            ],
        );
    }
}

/// Implements the execution of \<send\> element.
impl ExecutableContent for SendParameters {
    /// If unable to dispatch, place "error.communication" in internal queue
    /// If target is not supported, place "error.execution" in internal queue
    fn execute(&self, datamodel: &mut dyn Datamodel, fsm: &Fsm) {
        let global_clone = datamodel.global().clone();

        let target =
            match datamodel.get_expression_alternative_value(&self.target, &self.target_expr) {
                Ok(value) => value,
                Err(_) => {
                    // Error -> abort
                    return;
                }
            };

        let event_name =
            match datamodel.get_expression_alternative_value(&self.event, &self.event_expr) {
                Ok(value) => value,
                Err(_) => {
                    // Error -> abort
                    return;
                }
            };

        let send_id = if self.name_location.is_empty() {
            self.name.clone()
        } else {
            match datamodel.get_by_location(self.name_location.as_str()) {
                None => {
                    // Error -> abort
                    return;
                }
                Some(id) => id.value.unwrap(),
            }
        };

        let mut data_map = HashMap::new();
        datamodel.evaluate_params(&self.params, &mut data_map);

        let content = datamodel.evaluate_content(&self.content);

        let delay_ms = if !self.delay_expr.is_empty() {
            match datamodel.execute(&self.delay_expr) {
                None => {
                    // Error -> Abort
                    return;
                }
                Some(delay) => parse_duration_to_milliseconds(&delay),
            }
        } else {
            self.delay_ms as i64
        };

        if delay_ms < 0 {
            // Delay is invalid -> Abort
            error!("Send: delay {} is negative", self.delay_expr);
            datamodel.internal_error_execution();
            return;
        }

        if delay_ms > 0 && target.eq(SCXML_TARGET_INTERNAL) {
            // Can't send via internal queue
            error!("Send: illegal delay for target {}", target);
            datamodel.internal_error_execution();
            return;
        }
        let type_result =
            datamodel.get_expression_alternative_value(&self.type_value, &self.type_expr);

        let type_val = match type_result {
            Ok(val) => val,
            Err(err) => {
                error!("Failed to evaluate send type: {}", err);
                datamodel.internal_error_execution();
                return;
            }
        };

        let mut type_val_str = type_val.as_str();
        if type_val_str.is_empty() {
            type_val_str = SCXML_EVENT_PROCESSOR;
        }

        match datamodel.get_io_processors().get(type_val_str) {
            Some(iop) => {
                let event = Event {
                    name: event_name.clone(),
                    etype: EventType::external,
                    sendid: send_id.clone(),
                    origin: None,
                    origin_type: None,
                    invoke_id: fsm.caller_invoke_id.clone(),
                    param_values: if data_map.is_empty() {
                        None
                    } else {
                        Some(data_map.clone())
                    },
                    content,
                };

                let mut iopc = iop.get_copy();

                info!("schedule {} for {}", event, delay_ms);

                fsm.schedule(delay_ms, move || {
                    info!("send '{}' to '{}'", event, target );
                    let _ignored = iopc.send(&global_clone, &target, event.clone());
                });
            }
            None => {
                // W3C:  If the SCXML Processor does not support the type that is specified,
                // it must place the event error.execution on the internal event queue.
                datamodel.internal_error_execution();
            }
        }
    }

    fn get_type(&self) -> &str {
        TYPE_SEND
    }

    fn trace(&self, tracer: &mut dyn ExecutableContentTracer, _fsm: &Fsm) {
        tracer.print_name_and_attributes(
            self,
            &[
                ("name_location", &self.name_location),
                ("name", &self.name),
                ("name", &self.name),
                ("event_expr", &self.event_expr),
                ("target", &self.target),
                ("target_expr", &self.target_expr),
                ("type", &self.type_value),
                ("type_expr", &self.type_expr),
                ("delay", &self.delay_ms.to_string()),
                ("delay_expr", &self.delay_expr),
                ("name_list", &self.name_list),
                ("content", &format!("{:?}", self.content)),
                ("params", &opt_vec_to_string(&self.params)),
            ],
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::executable_content::parse_duration_to_milliseconds;

    #[test]
    fn delay_parse() {
        assert_eq!(parse_duration_to_milliseconds(&"6.7s".to_string()), 6700);
        assert_eq!(
            parse_duration_to_milliseconds(&"0.5d".to_string()),
            12 * 60 * 60 * 1000
        );
        assert_eq!(parse_duration_to_milliseconds(&"1m".to_string()), 60 * 1000);
        assert_eq!(parse_duration_to_milliseconds(&"0.001s".to_string()), 1);
        assert_eq!(parse_duration_to_milliseconds(&"6.7S".to_string()), 6700);
        assert_eq!(
            parse_duration_to_milliseconds(&"0.5D".to_string()),
            12 * 60 * 60 * 1000
        );
        assert_eq!(parse_duration_to_milliseconds(&"1M".to_string()), 60 * 1000);
        assert_eq!(parse_duration_to_milliseconds(&"0.001S".to_string()), 1);

        assert_eq!(parse_duration_to_milliseconds(&"x1S".to_string()), -1);
        assert_eq!(parse_duration_to_milliseconds(&"1Sx".to_string()), -1);
    }
}

/// a duration.
/// RegExp: "\\d*(\\.\\d+)?(ms|s|m|h|d))").
pub fn parse_duration_to_milliseconds(d: &String) -> i64 {
    lazy_static! {
        static ref DURATION_RE: Regex =
            Regex::new(r"^(\d*(\.\d+)?)(MS|S|M|H|D|ms|s|m|h|d)$").unwrap();
    }
    if d.is_empty() {
        0
    } else {
        let caps = DURATION_RE.captures(d);
        if caps.is_none() {
            -1
        } else {
            let cap = caps.unwrap();
            let value = cap.get(1).map_or("", |m| m.as_str());
            let unit = cap.get(3).map_or("", |m| m.as_str());

            if value.is_empty() {
                0
            } else {
                let mut v: f64 = value.parse::<f64>().unwrap();
                match unit {
                    "D" | "d" => {
                        v = v * 24.0 * 60.0 * 60.0 * 1000.0;
                    }
                    "H" | "h" => {
                        v = v * 60.0 * 60.0 * 1000.0;
                    }
                    "M" | "m" => {
                        v = v * 60000.0;
                    }
                    "S" | "s" => {
                        v = v * 1000.0;
                    }
                    "MS" | "ms" => {}
                    _ => {
                        return -1;
                    }
                }
                v.round() as i64
            }
        }
    }
}

pub struct DefaultExecutableContentTracer {
    trace_depth: usize,
}

impl DefaultExecutableContentTracer {
    pub fn new() -> DefaultExecutableContentTracer {
        DefaultExecutableContentTracer { trace_depth: 0 }
    }

    pub fn trace(&self, msg: &str) {
        info!("{:1$}{2}", " ", 2 * self.trace_depth, msg);
    }
}

impl ExecutableContentTracer for DefaultExecutableContentTracer {
    fn print_name_and_attributes(&mut self, ec: &dyn ExecutableContent, attrs: &[(&str, &String)]) {
        let mut buf = String::new();

        buf.push_str(format!("{:1$}{2} [", " ", 2 * self.trace_depth, ec.get_type()).as_str());

        let mut first = true;
        for (name, value) in attrs {
            if !value.is_empty() {
                if first {
                    first = false;
                } else {
                    buf.push(',');
                }
                buf.push_str(format!("{}:{}", name, value).as_str());
            }
        }
        buf.push_str("]");

        self.trace(&buf);
    }

    fn print_sub_content(&mut self, name: &str, fsm: &Fsm, content_id: ExecutableContentId) {
        self.trace(format!("{:1$}{2} {{", " ", 2 * self.trace_depth, name).as_str());
        self.trace_depth += 1;
        match fsm.executableContent.get(&content_id) {
            Some(vec) => {
                for ec in vec {
                    ec.trace(self, fsm);
                }
            }
            None => {}
        }
        self.trace_depth -= 1;
        self.trace(format!("{:1$}}}", " ", 2 * self.trace_depth).as_str());
    }
}
