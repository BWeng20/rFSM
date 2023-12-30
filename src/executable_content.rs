//! Implementation of "executable content" elements
//! See [W3C:Executable Content](https://www.w3.org/TR/scxml/#executable)

use std::any::Any;
use std::fmt::{Debug, Formatter};

use lazy_static::lazy_static;
use log::{info, warn};
use regex::Regex;

use crate::{Event, EventType};
use crate::fsm::{Datamodel, ExecutableContentId, Fsm};

pub const TARGET_INTERNAL: &str = "_internal";
pub const TARGET_SCXMLEVENT_PROCESSOR: &str = "http://www.w3.org/TR/scxml/#SCXMLEventProcessor";

pub const TYPE_IF: &str = "if";
pub const TYPE_EXPRESSION: &str = "expression";
pub const TYPE_SCRIPT: &str = "script";
pub const TYPE_LOG: &str = "log";
pub const TYPE_FOREACH: &str = "foreach";
pub const TYPE_SEND: &str = "send";
pub const TYPE_RAISE: &str = "raise";
pub const TYPE_CANCEL: &str = "cancel";
pub const TYPE_ASSIGN: &str = "assign";

pub trait ToAny: 'static {
    fn as_any(&mut self) -> &mut dyn Any;
}

impl<T: Debug + 'static> ToAny for T {
    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

pub trait ExecutableContent: ToAny + Debug + Send {
    fn execute(&self, datamodel: &mut dyn Datamodel, fsm: &Fsm);
    fn get_type(&self) -> &str;
    fn trace(&self, tracer: &mut dyn ExecutableContentTracer, fsm: &Fsm);
}

pub fn get_safe_executable_content_as<T: 'static>(ec: &mut dyn ExecutableContent) -> &mut T {
    let va = ec.as_any();
    match (*va).downcast_mut::<T>() {
        Some(v) => {
            v
        }
        None => {
            panic!("Failed to cast executable content")
        }
    }
}


pub fn get_executable_content_as<T: 'static>(ec: &mut dyn ExecutableContent) -> Option<&mut T> {
    let va = ec.as_any();
    match (*va).downcast_mut::<T>() {
        Some(v) => {
            Some(v)
        }
        None => {
            None
        }
    }
}


pub fn get_opt_executable_content_as<T: 'static>(ec_opt: Option<&mut dyn ExecutableContent>) -> Option<&mut T> {
    match ec_opt {
        Some(ec) => {
            get_executable_content_as::<T>(ec)
        }
        None => { None }
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

pub struct SendParameters {
    pub namelocation: String,
    /// The SCXML id.
    pub name: String,
    pub event: String,
    pub eventexpr: String,
    pub target: String,
    pub targetexpr: String,
    pub type_value: String,
    pub typeexpr: String,
    pub delay: String,
    pub delayexpr: String,
    pub name_list: String,

    /// content inside \<content\> child
    pub content: String,
    /// expr-attribute of \<content\> child
    pub content_expr: String,
}

pub struct Cancel {
    pub sendid: String,
    pub sendidexpr: String,
}

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
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Assign")
            .field("location", &self.location)
            .field("expr", &self.expr)
            .finish()
    }
}

impl ExecutableContent for Assign {
    fn execute(&self, _datamodel: &mut dyn Datamodel, _fsm: &Fsm) {
        todo!()
    }

    fn get_type(&self) -> &str {
        TYPE_ASSIGN
    }

    fn trace(&self, _tracer: &mut dyn ExecutableContentTracer, _fsm: &Fsm) {
        todo!()
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
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Raise")
            .field("event", &self.event)
            .finish()
    }
}

impl ExecutableContent for Raise {
    fn execute(&self, _datamodel: &mut dyn Datamodel, _fsm: &Fsm) {
        todo!()
    }

    fn get_type(&self) -> &str {
        TYPE_RAISE
    }

    fn trace(&self, _tracer: &mut dyn ExecutableContentTracer, _fsm: &Fsm) {
        todo!()
    }
}

impl Cancel {
    pub fn new() -> Cancel {
        Cancel {
            sendid: String::new(),
            sendidexpr: String::new(),
        }
    }
}

impl Debug for Cancel {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cancel")
            .field("sendid", &self.sendid)
            .field("sendidexpr", &self.sendidexpr)
            .finish()
    }
}

impl ExecutableContent for Cancel {
    fn execute(&self, _datamodel: &mut dyn Datamodel, _fsm: &Fsm) {
        todo!()
    }

    fn get_type(&self) -> &str {
        TYPE_CANCEL
    }

    fn trace(&self, _tracer: &mut dyn ExecutableContentTracer, _fsm: &Fsm) {
        todo!()
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

    fn trace(&self, _tracer: &mut dyn ExecutableContentTracer, _fsm: &Fsm) {
        todo!()
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
    fn execute(&self, datamodel: &mut dyn Datamodel, fsm: &Fsm) {
        let _l = datamodel.execute(fsm, &self.content);
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
    fn execute(&self, datamodel: &mut dyn Datamodel, fsm: &Fsm) {
        let l = datamodel.execute(fsm, &self.expression);
        datamodel.log(&l);
    }

    fn get_type(&self) -> &str {
        TYPE_LOG
    }

    fn trace(&self, tracer: &mut dyn ExecutableContentTracer, _fsm: &Fsm) {
        tracer.print_name_and_attributes(self, &[
            ("expression", &self.expression)]);
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
        match datamodel.executeCondition(fsm, &self.condition) {
            Ok(r) => {
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
            Err(e) => {
                warn!("Condition {} can't be evaluated. {}", self.condition, e);
            }
        }
    }

    fn get_type(&self) -> &str {
        TYPE_IF
    }

    fn trace(&self, tracer: &mut dyn ExecutableContentTracer, fsm: &Fsm) {
        tracer.print_name_and_attributes(self, &[
            ("condition", &self.condition)
        ]);
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
        let idx = if self.index.is_empty()
        {
            INDEX_TEMP.to_string()
        } else {
            self.index.clone()
        };
        datamodel.executeForEach(fsm, &self.array, &self.item, &idx, &mut |datamodel| {
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
        tracer.print_name_and_attributes(self, &[
            ("array", &self.array),
            ("item", &self.item),
            ("index", &self.index)]);
        tracer.print_sub_content("content", fsm, self.content);
    }
}

impl SendParameters {
    pub fn new() -> SendParameters {
        SendParameters {
            namelocation: "".to_string(),
            name: "".to_string(),
            event: "".to_string(),
            eventexpr: "".to_string(),
            target: "".to_string(),
            targetexpr: "".to_string(),
            type_value: "".to_string(),
            typeexpr: "".to_string(),
            delay: "".to_string(),
            delayexpr: "".to_string(),
            name_list: "".to_string(),
            content: "".to_string(),
            content_expr: "".to_string(),
        }
    }
}

impl Debug for SendParameters {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Send")
            .field("name", &self.name)
            .finish()
    }
}

/// Implements the excution of \<send\> element.
impl ExecutableContent for SendParameters {
    /// If unable to dispatch, place "error.communication" in internal queue
    /// If target is not supported, place "error.execution" in internal queue
    fn execute(&self, datamodel: &mut dyn Datamodel, fsm: &Fsm) {
        println!("Executing SEND");
        let target;
        if self.target.is_empty()
        {
            if !self.targetexpr.is_empty() {
                target = datamodel.execute(fsm, &self.targetexpr);
            } else {
                target = "".to_string();
            }
        } else {
            target = self.target.clone();
        };

        let event_name;
        if self.event.is_empty()
        {
            if !self.eventexpr.is_empty() {
                event_name = datamodel.execute(fsm, &self.eventexpr);
            } else {
                event_name = "".to_string();
            }
        } else {
            event_name = self.event.clone();
        };

        let sender = datamodel.global().externalQueue.sender.clone();

        if target.is_empty()
        {
            datamodel.global().internalQueue.enqueue(Event::error("execution"));
        } else {
            let delay;
            if !self.delayexpr.is_empty() {
                delay = datamodel.execute(fsm, &self.delayexpr);
            } else {
                delay = self.delay.clone();
            }
            if delay.is_empty() {
                todo!()
            } else {
                if target.eq(TARGET_INTERNAL) {
                    // Can't send timers via internal queue
                    datamodel.global().internalQueue.enqueue(Event::error("execution"));
                } else {
                    let delay_ms = parse_duration_to_millies(&delay);
                    if delay_ms <= 0
                    {
                        // Delay is invalid
                        datamodel.global().internalQueue.enqueue(Event::error("execution"));
                    } else {
                        fsm.schedule(delay_ms, move || {
                            // @TODO: fill all fields correctly!
                            let event = Box::new(Event {
                                name: event_name.clone(),
                                etype: EventType::external,
                                sendid: 0,
                                origin: "".to_string(),
                                origintype: "".to_string(),
                                invokeid: 0,
                                data: None,
                            });
                            println!(" Send {}", event.name);
                            let _ignored = sender.send(event);
                        });
                        println!("Scheduled Send (delay {}ms)", delay_ms);
                    }
                }
            }
        }
    }

    fn get_type(&self) -> &str {
        TYPE_SEND
    }

    fn trace(&self, tracer: &mut dyn ExecutableContentTracer, _fsm: &Fsm) {
        tracer.print_name_and_attributes(self, &[
            ("namelocation", &self.namelocation),
            ("name", &self.name),
            ("name", &self.name),
            ("eventexpr", &self.eventexpr),
            ("target", &self.target),
            ("targetexpr", &self.targetexpr),
            ("type", &self.type_value),
            ("typeexpr", &self.typeexpr),
            ("delay", &self.delay),
            ("delayexpr", &self.delayexpr),
            ("name_list", &self.name_list),
            ("content", &self.content)
        ]);
    }
}


#[cfg(test)]
mod tests {
    use crate::executable_content::parse_duration_to_millies;

    #[test]
    fn delay_parse() {
        assert_eq!(parse_duration_to_millies(&"6.7s".to_string()), 6700);
        assert_eq!(parse_duration_to_millies(&"0.5d".to_string()), 12 * 60 * 60 * 1000);
        assert_eq!(parse_duration_to_millies(&"1m".to_string()), 60 * 1000);
        assert_eq!(parse_duration_to_millies(&"0.001s".to_string()), 1);
        assert_eq!(parse_duration_to_millies(&"6.7S".to_string()), 6700);
        assert_eq!(parse_duration_to_millies(&"0.5D".to_string()), 12 * 60 * 60 * 1000);
        assert_eq!(parse_duration_to_millies(&"1M".to_string()), 60 * 1000);
        assert_eq!(parse_duration_to_millies(&"0.001S".to_string()), 1);

        assert_eq!(parse_duration_to_millies(&"x1S".to_string()), -1);
        assert_eq!(parse_duration_to_millies(&"1Sx".to_string()), -1);
    }
}


/// a duration.
/// RegExp: "\\d*(\\.\\d+)?(ms|s|m|h|d))").
pub fn parse_duration_to_millies(d: &String) -> i64 {
    lazy_static! {
        static ref DURATION_RE: Regex = Regex::new(r"^(\d*(\.\d+)?)(MS|S|M|H|D|ms|s|m|h|d)$").unwrap();
    }

    let caps = DURATION_RE.captures(d);
    if caps.is_none() {
        -1
    } else {
        let cap = caps.unwrap();
        let value = cap.get(1).map_or("", |m| m.as_str());
        let unit = cap.get(3).map_or("", |m| m.as_str());

        if value.is_empty() {
            -1
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

pub struct DefaultExecutableContentTracer {
    trace_depth: usize,

}

impl DefaultExecutableContentTracer {
    pub fn new() -> DefaultExecutableContentTracer {
        DefaultExecutableContentTracer {
            trace_depth: 0,
        }
    }

    pub fn trace(&self, msg: &str)
    {
        info!("{:1$}{2}"," ", 2 * self.trace_depth, msg);
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
