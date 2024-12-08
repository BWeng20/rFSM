//! Implements a SAX Parser for SCXML documents according to the W3C recommendation.
//! See [W3C:SCXML Overview](/doc/W3C_SCXML_2024_07_13/index.html#overview).

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

#[cfg(feature = "Debug_Reader")]
#[cfg(test)]
use std::println as debug;

use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, mem, str, string::String};

use crate::datamodel::{create_data_arc, Data};
use crate::ArgOption;
#[cfg(feature = "Debug_Reader")]
#[cfg(not(test))]
use log::debug;
use log::info;
use quick_xml::events::attributes::Attributes;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::executable_content::{
    get_opt_executable_content_as, get_safe_executable_content_as, parse_duration_to_milliseconds, Assign, Cancel,
    ExecutableContent, Expression, ForEach, If, Log, Raise, SendParameters,
};
use crate::fsm::push_param;
#[cfg(feature = "Debug_Reader")]
use crate::fsm::vec_to_string;
use crate::fsm::{
    map_history_type, map_transition_type, BindingType, DoneData, ExecutableContentId, Fsm, HistoryType, Invoke,
    Parameter, State, StateId, Transition, TransitionId, TransitionType, ID_COUNTER,
};

use crate::fsm::CommonContent;

pub type AttributeMap = HashMap<String, String>;
pub type XReader<'a> = Reader<&'a [u8]>;

static DOC_ID_COUNTER: AtomicU32 = AtomicU32::new(1);

pub static INCLUDE_PATH_ARGUMENT_OPTION: ArgOption = ArgOption {
    name: "includePaths",
    with_value: true,
    required: false,
};

pub fn include_path_from_arguments(named_arguments: &HashMap<&'static str, String>) -> Vec<PathBuf> {
    let mut include_paths = Vec::new();
    match named_arguments.get(INCLUDE_PATH_ARGUMENT_OPTION.name) {
        None => {}
        Some(paths) => {
            for pa in env::split_paths(&paths) {
                include_paths.push(pa.to_owned());
            }
        }
    }
    include_paths
}

/// *W3C says*:
/// The top-level wrapper element, which carries version information. The actual state machine consists of its children.
/// Note that only one of the children is active at any one time. See 3.11 Legal State Configurations and Specifications for details.\
/// *Attributes:*
/// + __initial__ A legal state specification. See 3.11 Legal State Configurations and Specifications for details. If not specified, the default initial state is the first child state in document order.
/// + __name__ Any valid NMTOKEN. The name of this state machine. It is for purely informational purposes.
/// + __xmlns__ The value MUST be '<http://www.w3.org/2005/07/scxml>'.
/// + __version__ Decimal, The value MUST be "1.0".
/// + __datamodel__ NMTOKEN, platform-specific, "null", "ecmascript", "xpath" or other platform-defined values.
/// + __binding__ "early" or "late", default is "early". See 5.3.3 Data Binding for details.
///
/// *Children:*
/// + __state__ A compound or atomic state. Occurs zero or more times. See 3.3 \<state\> for details.
/// + __parallel__  A parallel state. Occurs zero or more times. See 3.4 \<parallel\> for details.
/// + __final__  A top-level final state in the state machine. Occurs zero or more times. The SCXML processor must terminate processing when the state machine reaches this state. See 3.7 \<final\> for details.
/// + __datamodel__  Defines part or all of the data model. Occurs 0 or 1 times. See 5.2 \<datamodel\>
/// + __script__ Provides scripting capability. Occurs 0 or 1 times. 5.8 \<script\>
pub const TAG_SCXML: &str = "scxml";
pub const ATTR_NAME: &str = "name";
pub const ATTR_BINDING: &str = "binding";

pub const ATTR_DATAMODEL: &str = "datamodel";

pub const TAG_DATAMODEL: &str = "datamodel";
pub const TAG_DATA: &str = "data";
pub const TAG_VERSION: &str = "version";
pub const TAG_INITIAL: &str = "initial";
pub const ATTR_ID: &str = "id";

/// *W3C says*:
/// Holds the representation of a state.
///
/// *Attributes*:
/// + __id__  valid id as defined in [XML Schema].The identifier for this state. See 3.14 IDs for details.
/// + __initial__ The id of the default initial state (or states) for this state. MUST NOT be specified in conjunction with the \<initial\> element. MUST NOT occur in atomic states.
///
/// *Children*:
/// + __onentry__ Optional element holding executable content to be run upon entering this state. Occurs 0 or more times.
/// + __onexit__ Optional element holding executable content to be run when exiting this state. Occurs 0 or more times.
/// + __transition__ Defines an outgoing transition from this state. Occurs 0 or more times.
/// + __initial__ In states that have substates, an optional child which identifies the default initial state.
///   Any transition which takes the parent state as its target will result in the state machine also taking the transition
///   contained inside the \<initial\> element.
/// + __state__ Defines a sequential substate of the parent state. Occurs 0 or more times.
/// + __parallel__ Defines a parallel substate. Occurs 0 or more times.
/// + __final__  Defines a final substate. Occurs 0 or more times.
/// + __history__ A child pseudo-state which records the descendant state(s) that the parent state was in the last time the system transitioned from the parent.
///   May occur 0 or more times.
/// + __datamodel__ Defines part or all of the data model. Occurs 0 or 1 times.
/// + __invoke__ Invokes an external service. Occurs 0 or more times.
///
/// \[__Definition__: An atomic state is a \<state> that has no \<state>, \<parallel> or \<final> children.]\
/// \[__Definition__: A compound state is a \<state> that has \<state>, \<parallel>, or \<final> children
/// (or a combination of these).]\
/// \[__Definition__: The default initial state(s) of a compound state are those specified by the 'initial' attribute or
/// \<initial\> element, if either is present. Otherwise it is the state's first child state in document order. ]\
/// In a conformant SCXML document, a compound state may specify either an "initial" attribute or an \<initial\> element, but not both.
/// See 3.6 \<initial\> for a discussion of the difference between the two notations.
///
pub const TAG_STATE: &str = "state";
pub const ATTR_INITIAL: &str = "initial";
pub const TAG_HISTORY: &str = "history";
pub const TAG_PARALLEL: &str = "parallel";
pub const TAG_FINAL: &str = "final";
pub const TAG_TRANSITION: &str = "transition";
pub const ATTR_COND: &str = "cond";
pub const TAG_EVENT: &str = "event";
pub const TAG_TYPE: &str = "type";
pub const TAG_ON_ENTRY: &str = "onentry";
pub const TAG_ON_EXIT: &str = "onexit";
pub const TAG_INVOKE: &str = "invoke";
pub const ATTR_SRCEXPR: &str = "srcexpr";
pub const ATTR_AUTOFORWARD: &str = "autoforward";

pub const TAG_FINALIZE: &str = "finalize";
pub const TAG_DONEDATA: &str = "donedata";

pub const TAG_INCLUDE: &str = "include";
pub const TAG_HREF: &str = "href";
pub const ATTR_PARSE: &str = "parse";
pub const ATTR_XPOINTER: &str = "xpointer";

/// Executable content
pub const TAG_RAISE: &str = "raise";

/// *W3C says*:
/// __\<send\>__ is used to send events and data to external systems, including external SCXML Interpreters, or to raise events in the current SCXML session.
///
/// *Attributes*:
/// + __event__      A string indicating the name of message being generated. Must not occur with 'eventexpr'. If the type is *http:\/\/www\.w3.org/TR/scxml/#SCXMLEventProcessor*, either
///                  this attribute or 'eventexpr' must be present.
/// + __eventexpr__  A dynamic alternative to 'event'. If this attribute is present, the SCXML Processor must evaluate it when the parent \<send\> element is evaluated and treat
///                  the result as if it had been entered as the value of 'event'.
///                  If the type is "*http:\/\/www\.w3.org/TR/scxml/#SCXMLEventProcessor*", either this attribute or 'event' must be present. Must not occur with 'event'.
/// + __target__     A valid target URI. The unique identifier of the message target that the platform should send the event to. Must not occur with 'targetexpr'.
///                  See \[6.2.4 The Target of Send](/doc/W3C_SCXML_2024_07_13/index.html#SendTargets) for details.
///                  See \[SCXMLEventProcessor](/doc/W3C_SCXML_2024_07_13/index.html#SCXMLEventProcessor) for details about predefined targets.
/// + __targetexpr__ An expression evaluating to a valid target URI A dynamic alternative to 'target'. If this attribute is present, the SCXML Processor must evaluate it when the parent \<send\> element is evaluated and treat the result as if it
///                  had been entered as the value of 'target'. Must not occur with 'target'.
/// + __type__       The URI that identifies the transport mechanism for the message. Must not occur with 'typeexpr'.
///                  See \[6.2.5 The Type of Send](/doc/W3C_SCXML_2024_07_13/index.html#SendTypes).
/// + __typeexpr__   A dynamic alternative to 'type'. If this attribute is present, the SCXML Processor must evaluate it when the parent \<send\> element is evaluated and treat the result as if it had been
///                  entered as the value of 'type'. Must not occur with 'type'.
/// + __id__         Any valid token A string literal to be used as the identifier for this instance of \<send\>. Must not occur with 'idlocation'.
/// + __idlocation__ Any location expression evaluating to a data model location in which a system-generated id can be stored. See below for details. Must not occur with 'id'.
/// + __delay__      A time designation as defined in CSS2 format (RegExp: "\\d*(\\.\\d+)?(ms|s|m|h|d))").
///                  Indicates how long the processor should wait before dispatching the message.
///                  Must not occur with 'delayexpr' or when the attribute 'target' has the value "_internal".
/// + __delayexpr__  A value expression which returns a time designation as defined in CSS2 format. A dynamic alternative to 'delay'. If this attribute is present, the SCXML
///                  Processor must evaluate it when the parent \<send\> element is evaluated and treat the result as if it had been entered as the value of 'delay'.
///                  Must not occur with 'delay' or when the attribute 'target' has the value "_internal".
/// + __namelist__   A space-separated list of one or more data model locations to be included as attribute/value pairs with the message. (The name of the location is the attribute
///                  and the value stored at the location is the value.).
///                  Must not be specified in conjunction with \<content\> element.
///
/// *Children*
/// + __param__ The SCXML Processor must evaluate this element when the parent \<send\> element is evaluated and pass the resulting data to the external service when the message
///             is delivered. Occurs 0 or more times.
/// + __content__ The SCXML Processor must evaluate this element when the parent \<send\> element is evaluated and pass the resulting data to the external service when the message
///             is delivered. Occurs 0 or 1 times.
///
/// A conformant SCXML document must specify exactly one of 'event', 'eventexpr' and \<content\>.
/// A conformant document must not specify "namelist" or \<param\> with \<content\>.\
/// The SCXML Processor must include all attributes and values provided by \<param\> or 'namelist'
/// even if duplicates occur.\
/// If 'idlocation' is present, the SCXML Processor must generate an id when the parent \<send\>
/// element is evaluated and store it in this location. See \[3.14 IDs](/doc/W3C_SCXML_2024_07_13/index.html#IDs) for details.\
/// If a delay is specified via 'delay' or 'delayexpr', the SCXML Processor must interpret the
/// character string as a time interval. It must dispatch the message only when the delay interval
/// elapses. (Note that the evaluation of the send tag will return immediately.)\
/// The Processor must evaluate all arguments to \<send\> when the \<send\> element is evaluated, and
/// not when the message is actually dispatched. If the evaluation of \<send\>'s arguments produces an error,
/// the Processor must discard the message without attempting to deliver it. If the SCXML session terminates
/// before the delay interval has elapsed, the SCXML Processor must discard the message without attempting
/// to deliver it.
pub const TAG_SEND: &str = "send";

pub const ATTR_EVENT: &str = "event";
pub const ATTR_EVENTEXPR: &str = "eventexpr";
pub const ATTR_TARGET: &str = "target";
pub const ATTR_TARGETEXPR: &str = "targetexpr";
pub const TARGET_INTERNAL: &str = "_internal";
pub const ATTR_TYPE: &str = "type";
pub const ATTR_TYPEEXPR: &str = "typeexpr";
pub const ATTR_IDLOCATION: &str = "idlocation";
pub const ATTR_DELAY: &str = "delay";
pub const ATTR_DELAYEXPR: &str = "delayexpr";
pub const ATTR_NAMELIST: &str = "namelist";
pub const TAG_PARAM: &str = "param";
pub const TAG_CONTENT: &str = "content";

pub const TAG_LOG: &str = "log";
pub const TAG_SCRIPT: &str = "script";
pub const ATTR_SRC: &str = "src";
pub const TAG_ASSIGN: &str = "assign";
pub const ATTR_LOCATION: &str = "location";

pub const TAG_IF: &str = "if";
pub const TAG_FOR_EACH: &str = "foreach";
pub const ATTR_ARRAY: &str = "array";
pub const ATTR_ITEM: &str = "item";
pub const ATTR_INDEX: &str = "index";

pub const TAG_CANCEL: &str = "cancel";
pub const ATTR_SENDIDEXPR: &str = "sendidexpr";
pub const ATTR_SENDID: &str = "sendid";

pub const TAG_ELSE: &str = "else";
pub const TAG_ELSEIF: &str = "elseif";

pub const ATTR_LABEL: &str = "label";
pub const ATTR_EXPR: &str = "expr";

pub const NS_XINCLUDE: &str = "http://www.w3.org/2001/XInclude";

struct ReaderStackItem {
    current_state: StateId,
    current_transition: TransitionId,
    current_tag: String,
}

impl ReaderStackItem {
    pub fn new(o: &ReaderStackItem) -> ReaderStackItem {
        ReaderStackItem {
            current_state: o.current_state,
            current_transition: o.current_transition,
            current_tag: o.current_tag.clone(),
        }
    }
}

struct ReaderState {
    // True if reader in inside an scxml element
    in_scxml: bool,
    id_count: i32,
    file: PathBuf,
    content: String,

    // The resulting fsm
    fsm: Box<Fsm>,

    current: ReaderStackItem,
    stack: Vec<ReaderStackItem>,
    executable_content_stack: Vec<(ExecutableContentId, &'static str)>,
    current_executable_content: ExecutableContentId,
    include_paths: Vec<PathBuf>,
}

impl ReaderState {
    pub fn new() -> ReaderState {
        ReaderState {
            in_scxml: false,
            id_count: 0,
            stack: vec![],
            executable_content_stack: vec![],
            current_executable_content: 0,
            current: ReaderStackItem {
                current_state: 0,
                current_transition: 0,
                current_tag: "".to_string(),
            },
            fsm: Box::new(Fsm::new()),
            file: Path::new("Buffer").to_path_buf(),
            content: "".to_string(),
            include_paths: Vec::new(),
        }
    }

    /// Process a XML file.
    /// For technical reasons (to handle user content) the file is read in a temporary buffer.
    fn process_file(&mut self, file: &Path) -> Result<&str, String> {
        self.file.clone_from(&file.to_path_buf());
        match File::open(self.file.clone()) {
            Ok(mut f) => {
                self.content.clear();
                match f.read_to_string(&mut self.content) {
                    Ok(_len) => self.process(),
                    Err(e) => Err(format!("Failed to read {:?}. {}", self.file, e)),
                }
            }
            Err(e) => Err(format!("Failed to open {:?}. {}", self.file, e)),
        }
    }

    /// Process all events from current content
    fn process(&mut self) -> Result<&str, String> {
        #[cfg(feature = "Debug_Reader")]

        debug!(">>> Reading {:?}", self.file);

        // @TODO: reader needs a mutable reference to "content", for processing user content we need a read-only-ref.
        // How we can share instead of "clone"?
        let ct = self.content.clone();
        let mut reader = Reader::from_str(ct.as_str());
        reader.config_mut().trim_text(true);

        let mut txt = Vec::new();
        loop {
            match reader.read_event() {
                Err(e) => {
                    #[cfg(feature = "Debug_Reader")]

                    debug!("<<< {:?}", self.file);
                    return Err(format!(
                        "Error at position {}: {:?}",
                        reader.buffer_position(),
                        e
                    ));
                }
                Ok(Event::Eof) => break,
                Ok(Event::Start(e)) => {
                    self.start_element(&mut reader, &e, true);
                }
                Ok(Event::End(e)) => {
                    self.end_element(str::from_utf8(e.local_name().as_ref()).unwrap());
                }
                Ok(Event::Empty(e)) => {
                    // Element without content.
                    self.start_element(&mut reader, &e, false);
                    self.end_element(str::from_utf8(e.local_name().as_ref()).unwrap());
                }
                Ok(Event::Text(e)) => txt.push(e.unescape().unwrap().into_owned()),
                Ok(Event::Comment(_e)) => {
                    #[cfg(feature = "Debug_Reader")]
                    debug!("Comment :{}", _e.unescape().unwrap())
                }

                // Ignore other
                Ok(_e) => {
                    #[cfg(feature = "Debug_Reader")]
                    debug!("Ignored SAX Event {:?}", _e)
                }
            }
        }
        #[cfg(feature = "Debug_Reader")]

        debug!("<<< {:?}", self.file);
        Ok("ok")
    }

    fn push(&mut self, tag: &str) {
        self.stack.push(ReaderStackItem::new(&self.current));
        self.current.current_tag = tag.to_string();
    }

    fn pop(&mut self) {
        let p = self.stack.pop();
        if p.is_some() {
            self.current = p.unwrap();
        }
    }

    fn generate_name(&mut self) -> String {
        self.id_count += 1;
        format!("__id{}", self.id_count)
    }

    fn parse_location_expressions(&mut self, location_expr: &str, targets: &mut Vec<String>) {
        location_expr.split_ascii_whitespace().for_each(|location| {
            targets.push(location.to_string());
        });
    }

    fn parse_state_specification(&mut self, target_name: &str, targets: &mut Vec<StateId>) {
        target_name
            .split_ascii_whitespace()
            .for_each(|target| targets.push(self.get_or_create_state(target, false)));
    }

    fn parse_boolean(&mut self, value: &Option<&String>, default: bool) -> bool {
        match value {
            Some(val) => val.eq_ignore_ascii_case("true"),
            None => default,
        }
    }

    fn get_state_by_id_mut(&mut self, id: StateId) -> &mut State {
        self.fsm.get_state_by_id_mut(id)
    }

    fn get_current_state(&mut self) -> &mut State {
        let id = self.current.current_state;
        if id == 0 {
            panic!("Internal error: Current State is unknown");
        }
        self.get_state_by_id_mut(id)
    }

    fn get_current_transition(&mut self) -> &mut Transition {
        let id = self.current.current_transition;
        if id == 0 {
            panic!("Internal error: Current Transition is unknown");
        }
        self.fsm.get_transition_by_id_mut(id)
    }

    /// Starts a new region of executable content.\
    /// A stack is used to handle nested executable content.
    /// This stack works independent from the main element stack, but should be
    /// considered as synchronized with it.
    /// # Arguments
    /// * `stack` - If true, the current region is put on stack,
    ///             continued after the matching [get_executable_content](Self::get_executable_content).
    ///             If false, the current stack is discarded.
    /// * `tag`   - Tag for which this region was started. USed to mark the region for later clean-up.
    fn start_executable_content_region(&mut self, stack: bool, tag: &'static str) -> ExecutableContentId {
        if stack {
            #[cfg(feature = "Debug_Reader")]

            debug!(
                " push executable content region #{} {}",
                self.current_executable_content, tag
            );
            self.executable_content_stack
                .push((self.current_executable_content, tag));
        } else {
            self.executable_content_stack.clear();
        }
        self.current_executable_content = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        #[cfg(feature = "Debug_Reader")]

        debug!(
            " start executable content region #{}",
            self.current_executable_content
        );
        self.fsm
            .executableContent
            .insert(self.current_executable_content, Vec::new());
        self.current_executable_content
    }

    /// Get the last entry for the current content region.
    fn get_last_executable_content_entry_for_region(
        &mut self,
        ec_id: ExecutableContentId,
    ) -> Option<&mut dyn ExecutableContent> {
        let v = self.fsm.executableContent.get_mut(&ec_id);
        match v {
            Some(vc) => Some(vc.last_mut().unwrap().as_mut()),
            None => None,
        }
    }

    /// Ends the current executable content region and returns the old region id.\
    /// The current id is reset to 0 or popped from stack if the stack is not empty.
    /// See [start_executable_content](Self::start_executable_content).
    fn end_executable_content_region(&mut self, tag: &'static str) -> ExecutableContentId {
        if self.current_executable_content == 0 {
            panic!("Try to get executable content in unsupported document part.");
        } else {
            let ec_id = self.current_executable_content;
            #[cfg(feature = "Debug_Reader")]

            debug!(" end executable content region #{}", ec_id);
            match self.executable_content_stack.pop() {
                Some((oec_id, oec_tag)) => {
                    self.current_executable_content = oec_id;
                    #[cfg(feature = "Debug_Reader")]

                    debug!(" pop executable content region #{} {}", oec_id, oec_tag);
                    if (!tag.is_empty()) && tag.ne(oec_tag) {
                        self.end_executable_content_region(tag);
                    }
                }
                None => {
                    self.current_executable_content = 0;
                }
            };
            if self.fsm.executableContent.contains_key(&ec_id) {
                ec_id
            } else {
                0
            }
        }
    }

    /// Adds content to the current executable content region.
    fn add_executable_content(&mut self, ec: Box<dyn ExecutableContent>) {
        if self.current_executable_content == 0 {
            panic!("Try to add executable content to unsupported document part.");
        } else {
            #[cfg(feature = "Debug_Reader")]

            debug!(
                "Adding Executable Content '{}' to #{}",
                ec.get_type(),
                self.current_executable_content
            );
            self.fsm
                .executableContent
                .get_mut(&self.current_executable_content)
                .unwrap()
                .push(ec);
        }
    }

    fn get_parent_tag(&self) -> &str {
        let mut r = "";
        if !self.stack.is_empty() {
            r = self.stack.last().as_ref().unwrap().current_tag.as_str();
        }
        r
    }

    pub fn verify_parent_tag(&self, name: &str, allowed_parents: &[&str]) -> &str {
        let parent_tag = self.get_parent_tag();
        if !allowed_parents.contains(&parent_tag) {
            let mut allowed_parents_s = "".to_string();
            let len = allowed_parents.len();
            for (i, ai) in allowed_parents.iter().enumerate() {
                allowed_parents_s += format!(
                    "{}<{}>",
                    if i > 0 {
                        if i < (len - 1) {
                            ", "
                        } else {
                            " or "
                        }
                    } else {
                        ""
                    },
                    ai
                )
                .as_str();
            }
            panic!(
                "<{}> inside <{}>. Only allowed inside {}",
                name, parent_tag, allowed_parents_s
            );
        }
        parent_tag
    }

    fn get_or_create_state(&mut self, name: &str, parallel: bool) -> StateId {
        let m = self.fsm.statesNames.get(name).cloned();
        match m {
            None => {
                let mut s = State::new(name);
                s.id = (self.fsm.states.len() + 1) as StateId;
                s.is_parallel = parallel;
                let sid = s.id;
                self.fsm.statesNames.insert(s.name.clone(), s.id); // s.id, s);
                self.fsm.states.push(s);
                sid
            }
            Some(id) => {
                if parallel {
                    self.fsm
                        .states
                        .get_mut((id - 1) as usize)
                        .unwrap()
                        .is_parallel = true;
                }
                id
            }
        }
    }

    fn get_or_create_state_with_attributes(&mut self, attr: &AttributeMap, parallel: bool, parent: StateId) -> StateId {
        let sname = match attr.get(ATTR_ID) {
            None => self.generate_name(),
            Some(id) => id.clone(),
        };
        let id = self.get_or_create_state(&sname, parallel);

        let initial;
        match attr.get(ATTR_INITIAL) {
            None => initial = 0,
            Some(id_refs) => {
                // Create initial-transition with the initial states
                let mut t = Transition::new();
                t.doc_id = DOC_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
                t.transition_type = TransitionType::Internal;
                t.source = id;
                initial = t.id;
                self.parse_state_specification(id_refs, &mut t.target);
                #[cfg(feature = "Debug_Reader")]

                debug!(
                    " {}#{}.initial = {} -> {}",
                    sname,
                    id,
                    initial,
                    vec_to_string(&t.target)
                );
                self.fsm.transitions.insert(t.id, t);
            }
        }

        let state = self.get_state_by_id_mut(id);
        if initial != 0 {
            state.initial = initial;
        }
        state.doc_id = DOC_ID_COUNTER.fetch_add(1, Ordering::Relaxed);

        if parent != 0 {
            state.parent = parent;
            let parent_state = self.get_state_by_id_mut(parent);
            #[cfg(feature = "Debug_Reader")]

            debug!(
                " state #{} {}{} parent {}",
                id,
                if parallel { "(parallel) " } else { "" },
                sname,
                parent_state.name
            );
            if !parent_state.states.contains(&id) {
                parent_state.states.push(id);
            }
        } else {
            #[cfg(feature = "Debug_Reader")]

            debug!(
                " state #{} {}{} no parent",
                id,
                if parallel { "(parallel) " } else { "" },
                sname
            );
        }

        id
    }

    fn get_required_attr<'a>(tag: &str, attribute: &str, attr: &'a AttributeMap) -> &'a String {
        let attr = attr.get(attribute);
        if attr.is_none() {
            panic!("<{}> requires attribute {}", tag, attribute);
        }
        attr.unwrap()
    }

    fn read_from_uri(&mut self, uri: &String) -> Result<String, String> {
        let url_result = reqwest::Url::parse(uri);
        match url_result {
            Ok(url) => {
                #[cfg(feature = "Debug_Reader")]

                debug!("read from URL {}", url);

                match url.scheme().to_ascii_lowercase().as_str() {
                    "file" => self.read_from_relative_path(url.path()),
                    &_ => {
                        let resp = reqwest::blocking::get(url);
                        match resp {
                            Ok(r) => match r.text() {
                                Ok(s) => Ok(s),
                                Err(e) => Err(format!("Failed to decode from {}. {}", uri, e)),
                            },
                            Err(e) => Err(format!("Failed to download {}. {}", uri, e)),
                        }
                    }
                }
            }
            Err(_e) => {
                #[cfg(feature = "Debug_Reader")]
                debug!(
                    "{} is not a URI ({}). Try loading as relative path...",
                    uri, _e
                );
                self.read_from_relative_path(uri.as_str())
            }
        }
    }

    fn read_from_relative_path(&mut self, path: &str) -> Result<String, String> {
        let file_src = self.get_resolved_path(path)?;
        self.file.clone_from(&file_src);
        match File::open(file_src.clone()) {
            Ok(mut file) => {
                let mut buf = String::with_capacity(file.metadata().unwrap().len() as usize);
                match file.read_to_string(&mut buf) {
                    Ok(_r) => Ok(buf),
                    Err(e) => Err(e.to_string()),
                }
            }
            Err(e) => Err(format!(
                "Failed to load relative path {:?}: {}",
                file_src, e
            )),
        }
    }

    /// A new "parallel" element started
    fn start_parallel(&mut self, attr: &AttributeMap) -> StateId {
        self.verify_parent_tag(TAG_PARALLEL, &[TAG_SCXML, TAG_STATE, TAG_PARALLEL]);
        let state_id = self.get_or_create_state_with_attributes(attr, true, self.current.current_state);
        self.current.current_state = state_id;
        state_id
    }

    /// A new "final" element started
    fn start_final(&mut self, attr: &AttributeMap) -> StateId {
        self.verify_parent_tag(TAG_FINAL, &[TAG_SCXML, TAG_STATE]);
        let state_id = self.get_or_create_state_with_attributes(attr, false, self.current.current_state);

        self.fsm.get_state_by_id_mut(state_id).is_final = true;
        self.current.current_state = state_id;
        state_id
    }

    /// A new "donedata" element started
    fn start_donedata(&mut self) {
        self.verify_parent_tag(TAG_DONEDATA, &[TAG_FINAL]);
        self.get_current_state().donedata = Some(DoneData::new());
    }

    /// A new "history" element started
    fn start_history(&mut self, attr: &AttributeMap) -> StateId {
        self.verify_parent_tag(TAG_HISTORY, &[TAG_STATE, TAG_PARALLEL]);
        // Don't add history-states to "states" (parent = 0)
        let state_id = self.get_or_create_state_with_attributes(attr, false, 0);
        if self.current.current_state > 0 {
            let parent_state = self.get_current_state();
            parent_state.history.push(state_id);
        }
        let hstate = self.fsm.get_state_by_id_mut(state_id);
        // Assign parent manually, as we didn't gave get_or_create_state_with_attributes the parent.
        hstate.parent = self.current.current_state;

        match attr.get(TAG_TYPE) {
            None => hstate.history_type = HistoryType::Shallow,
            Some(type_name) => hstate.history_type = map_history_type(type_name),
        }
        self.current.current_state = state_id;
        state_id
    }

    // A new "state" element started
    fn start_state(&mut self, attr: &AttributeMap) -> StateId {
        self.verify_parent_tag(TAG_STATE, &[TAG_SCXML, TAG_STATE, TAG_PARALLEL]);
        let sid = self.get_or_create_state_with_attributes(attr, false, self.current.current_state);
        self.current.current_state = sid;
        sid
    }

    // A "datamodel" element started (node, not attribute)
    fn start_datamodel(&mut self) {
        self.verify_parent_tag(TAG_DATAMODEL, &[TAG_SCXML, TAG_STATE, TAG_PARALLEL]);
    }

    fn start_data(&mut self, attr: &AttributeMap, reader: &mut XReader, has_content: bool) {
        self.verify_parent_tag(TAG_DATA, &[TAG_DATAMODEL]);

        let id = Self::get_required_attr(TAG_DATA, ATTR_ID, attr);
        let src = attr.get(ATTR_SRC);

        let expr = attr.get(ATTR_EXPR);

        let content = if has_content {
            self.read_content(TAG_DATA, reader)
        } else {
            String::new()
        };

        // W3C:
        // In a conformant SCXML document, a \<data}> element may have either a 'src' or an 'expr' attribute,
        // but must not have both. Furthermore, if either attribute is present, the element must not have any children.
        // Thus 'src', 'expr' and children are mutually exclusive in the <data> element.

        let data_value = if src.is_some() {
            if !(expr.is_none() && content.is_empty()) {
                panic!(
                    "{} shall have only {}, {} or children, but not some combination of it.",
                    TAG_DATA, ATTR_SRC, ATTR_EXPR
                );
            }

            // W3C:
            // Gives the location from which the data object should be fetched.
            // If the 'src' attribute is present, the Platform must fetch the specified object
            // at the time specified by the 'binding' attribute of \<scxml\> and must assign it as
            // the value of the data element

            match self.read_from_uri(src.unwrap()) {
                Ok(source) => {
                    #[cfg(feature = "Debug_Reader")]

                    debug!("src='{}':\n{}", src.unwrap(), source);
                    source
                }
                Err(e) => {
                    panic!("Can't read data source '{}'. {}", src.unwrap(), e);
                }
            }
        } else if expr.is_some() {
            if !content.is_empty() {
                panic!(
                    "{} shall have only {}, {} or children, but not some combination of it.",
                    TAG_DATA, ATTR_SRC, ATTR_EXPR
                );
            }
            expr.unwrap().clone()
        } else if !content.is_empty() {
            content
        } else {
            "".to_string()
        };
        self.get_current_state()
            .data
            .insert(id.to_string(), create_data_arc(Data::Source(data_value)));
    }

    /// A "initial" element started (the element, not the attribute)
    fn start_initial(&mut self) {
        self.verify_parent_tag(TAG_INITIAL, &[TAG_STATE, TAG_PARALLEL]);
        if self.get_current_state().initial > 0 {
            panic!(
                "<{}> must not be specified if {}-attribute was given",
                TAG_INITIAL, ATTR_INITIAL
            )
        }
    }

    fn start_invoke(&mut self, attr: &AttributeMap) {
        let _parent_tag = self
            .verify_parent_tag(TAG_INVOKE, &[TAG_STATE, TAG_PARALLEL])
            .to_string();
        let mut invoke = Invoke::new();

        if let Some(type_opt) = attr.get(ATTR_TYPE) {
            invoke.type_name = Data::Source(type_opt.clone());
        }
        if let Some(typeexpr) = attr.get(ATTR_TYPEEXPR) {
            invoke.type_expr = Data::Source(typeexpr.clone());
        }

        // W3c: Must not occur with the 'srcexpr' attribute or the <content> element.
        if let Some(src) = attr.get(ATTR_SRC) {
            invoke.src = Data::Source(src.clone());
        }
        if let Some(srcexpr) = attr.get(ATTR_SRCEXPR) {
            invoke.src_expr = Data::Source(srcexpr.to_string());
        }

        // TODO--
        if let Some(id) = attr.get(ATTR_ID) {
            invoke.invoke_id.clone_from(id);
        }

        invoke
            .parent_state_name
            .clone_from(&self.get_current_state().name);

        if let Some(idlocation) = attr.get(ATTR_IDLOCATION) {
            invoke.external_id_location.clone_from(idlocation);
        }

        if let Some(name_list) = attr.get(ATTR_NAMELIST) {
            self.parse_location_expressions(name_list, &mut invoke.name_list);
        }
        invoke.autoforward = self.parse_boolean(&attr.get(ATTR_AUTOFORWARD), false);

        self.get_current_state().invoke.push(invoke);
    }

    fn start_finalize(&mut self, _attr: &AttributeMap) {
        let _parent_tag = self
            .verify_parent_tag(TAG_FINALIZE, &[TAG_INVOKE])
            .to_string();
        self.start_executable_content_region(false, TAG_FINALIZE);
    }

    fn end_finalize(&mut self) {
        let ec_id = self.end_executable_content_region(TAG_FINALIZE);
        self.get_current_state().invoke.last_mut().finalize = ec_id;
    }

    fn start_transition(&mut self, attr: &AttributeMap) {
        let parent_tag = self
            .verify_parent_tag(
                TAG_TRANSITION,
                &[TAG_HISTORY, TAG_INITIAL, TAG_STATE, TAG_PARALLEL],
            )
            .to_string();

        let mut t = Transition::new();
        t.doc_id = DOC_ID_COUNTER.fetch_add(1, Ordering::Relaxed);

        // Start script.
        self.start_executable_content_region(false, TAG_TRANSITION);

        let event = attr.get(TAG_EVENT);
        if event.is_some() {
            t.events = event
                .unwrap()
                .split_whitespace()
                .map(|s| {
                    // Strip redundant "." and ".*" suffix
                    let mut rt = s;
                    let mut do_it = true;
                    while do_it {
                        do_it = false;
                        match rt.strip_suffix(".*") {
                            None => {}
                            Some(r) => {
                                do_it = true;
                                rt = r
                            }
                        }
                        match rt.strip_suffix(".") {
                            None => {}
                            Some(r) => {
                                do_it = true;
                                rt = r
                            }
                        }
                    }
                    rt.to_string()
                })
                .collect();
            t.wildcard = t.events.contains(&"*".to_string());
        }

        let cond = attr.get(ATTR_COND);
        if cond.is_some() {
            t.cond = Data::Source(cond.unwrap().clone());
        }

        let target = attr.get(ATTR_TARGET);
        match target {
            None => (),
            Some(target_name) => {
                self.parse_state_specification(target_name, &mut t.target);
            }
        }

        let trans_type = attr.get(TAG_TYPE);
        if trans_type.is_some() {
            t.transition_type = map_transition_type(trans_type.unwrap())
        }

        let state = self.get_current_state();

        if parent_tag.eq(TAG_INITIAL) {
            if state.initial > 0 {
                panic!("<initial> must not be specified if initial-attribute was given")
            }
            #[cfg(feature = "Debug_Reader")]

            debug!(" {}#{}.initial = {}", state.name, state.id, t.id);
            state.initial = t.id;
        } else {
            state.transitions.push(t.id);
        }
        t.source = state.id;
        self.current.current_transition = t.id;
        self.fsm.transitions.insert(t.id, t);
    }

    fn end_transition(&mut self) {
        let ec_id = self.end_executable_content_region(TAG_TRANSITION);
        let trans = self.get_current_transition();
        // Assign the collected content to the transition.
        trans.content = ec_id;
    }

    fn start_script(&mut self, attr: &AttributeMap, reader: &mut XReader, has_content: bool) {
        let at_root = self.get_parent_tag().eq(TAG_SCXML);

        if !at_root {
            self.verify_parent_tag(
                TAG_SCRIPT,
                &[
                    TAG_SCXML,
                    TAG_TRANSITION,
                    TAG_ON_EXIT,
                    TAG_ON_ENTRY,
                    TAG_IF,
                    TAG_FOR_EACH,
                    TAG_FINALIZE,
                ],
            );
        };

        if at_root {
            self.start_executable_content_region(false, TAG_SCRIPT);
        }

        let mut s = Expression::new();

        let src = attr.get(ATTR_SRC);
        if src.is_some() {
            let file_src = src.unwrap();
            // W3C:
            // If the script can not be downloaded within a platform-specific timeout interval,
            // the document is considered non-conformant, and the platform must reject it.
            match self.read_from_uri(file_src) {
                Ok(source) => {
                    #[cfg(feature = "Debug_Reader")]
                    debug!("src='{}':\n{}", file_src, source);
                    s.content = Data::Source(source);
                }
                Err(e) => {
                    panic!("Can't read script '{}'. {}", file_src, e);
                }
            }
        }

        let script_text = if has_content {
            self.read_content(TAG_SCRIPT, reader)
        } else {
            String::new()
        };

        let src = script_text.trim();

        if !src.is_empty() {
            if !s.content.is_empty() {
                panic!("<script> with 'src' attribute shall not have content.")
            }
            s.content = Data::Source(src.to_string());
        }

        self.add_executable_content(Box::new(s));
        if at_root {
            self.fsm.script = self.end_executable_content_region(TAG_SCRIPT);
        }
    }

    fn start_for_each(&mut self, attr: &AttributeMap) {
        self.verify_parent_tag(
            TAG_FOR_EACH,
            &[
                TAG_ON_ENTRY,
                TAG_ON_EXIT,
                TAG_TRANSITION,
                TAG_FOR_EACH,
                TAG_IF,
                TAG_FINALIZE,
            ],
        );

        let ec_id = self.current_executable_content;
        let mut fe = ForEach::new();
        fe.array
            .clone_from(Self::get_required_attr(TAG_FOR_EACH, ATTR_ARRAY, attr));
        fe.item
            .clone_from(Self::get_required_attr(TAG_FOR_EACH, ATTR_ITEM, attr));
        if let Some(index) = attr.get(ATTR_INDEX) {
            fe.index.clone_from(index);
        }
        self.add_executable_content(Box::new(fe));
        let content_id = self.start_executable_content_region(true, TAG_FOR_EACH);

        let ec_opt = self.get_last_executable_content_entry_for_region(ec_id);
        match get_opt_executable_content_as::<ForEach>(ec_opt) {
            Some(fe) => {
                fe.content = content_id;
            }
            None => {
                panic!(
                    "Internal Error: Executable Content missing in start_for_each in region #{}",
                    ec_id
                );
            }
        }
    }

    fn end_for_each(&mut self) {
        self.end_executable_content_region(TAG_FOR_EACH);
    }

    fn start_cancel(&mut self, attr: &AttributeMap) {
        self.verify_parent_tag(
            TAG_CANCEL,
            &[
                TAG_TRANSITION,
                TAG_ON_EXIT,
                TAG_ON_ENTRY,
                TAG_IF,
                TAG_FOR_EACH,
            ],
        );

        let sendid = attr.get(ATTR_SENDID);
        let sendidexpr = attr.get(ATTR_SENDIDEXPR);

        let mut cancel = Cancel::new();

        if let Some(sendid_value) = sendid {
            if sendidexpr.is_some() {
                panic!(
                    "{}: attributes {} and {} must not occur both",
                    TAG_CANCEL, ATTR_SENDID, ATTR_SENDIDEXPR
                );
            }
            cancel.send_id.clone_from(sendid_value);
        } else if let Some(sendidexpr_value) = sendidexpr {
            cancel.send_id_expr = Data::Source(sendidexpr_value.clone());
        } else {
            panic!(
                "{}: attribute {} or {} must be given",
                TAG_CANCEL, ATTR_SENDID, ATTR_SENDIDEXPR
            );
        }
        self.add_executable_content(Box::new(cancel));
    }

    fn start_on_entry(&mut self, _attr: &AttributeMap) {
        self.verify_parent_tag(TAG_ON_ENTRY, &[TAG_STATE, TAG_PARALLEL, TAG_FINAL]);
        self.start_executable_content_region(false, TAG_ON_ENTRY);
    }

    fn end_on_entry(&mut self) {
        let ec_id = self.end_executable_content_region(TAG_ON_ENTRY);
        let state = self.get_current_state();
        // Add the collected content to on-entry.
        state.onentry.push(ec_id);
    }

    fn start_on_exit(&mut self, _attr: &AttributeMap) {
        self.verify_parent_tag(TAG_ON_EXIT, &[TAG_STATE, TAG_PARALLEL, TAG_FINAL]);
        self.start_executable_content_region(false, TAG_ON_EXIT);
    }

    fn end_on_exit(&mut self) {
        let ec_id = self.end_executable_content_region(TAG_ON_EXIT);
        let state = self.get_current_state();
        // Add the collected content to the on-exit.
        state.onexit.push(ec_id);
    }

    fn start_if(&mut self, attr: &AttributeMap) {
        self.verify_parent_tag(
            TAG_IF,
            &[
                TAG_ON_ENTRY,
                TAG_ON_EXIT,
                TAG_TRANSITION,
                TAG_FOR_EACH,
                TAG_IF,
                TAG_FINALIZE,
            ],
        );

        let ec_if = If::new(Self::get_required_attr(TAG_IF, ATTR_COND, attr));
        self.add_executable_content(Box::new(ec_if));
        let if_id = self.current_executable_content;

        self.start_executable_content_region(true, TAG_IF);
        let if_cid = self.current_executable_content;

        let if_ec = self.get_last_executable_content_entry_for_region(if_id);
        match get_opt_executable_content_as::<If>(if_ec) {
            Some(evc_if) => {
                evc_if.content = if_cid;
            }
            None => {
                panic!(
                    "Internal Error: Executable Content missing in start_if in region #{}",
                    if_id
                );
            }
        }
    }

    fn end_if(&mut self) {
        let _content_id = self.end_executable_content_region(TAG_IF);
    }

    fn start_else_if(&mut self, attr: &AttributeMap) {
        self.verify_parent_tag(TAG_ELSEIF, &[TAG_IF]);

        // Close parent <if> content region
        self.end_executable_content_region(TAG_IF);

        let mut if_id = self.current_executable_content;

        // Start new "else" region - will contain only one "if", replacing current "if" stack element.
        self.start_executable_content_region(true, TAG_IF);
        let else_id = self.current_executable_content;

        // Add new "if"
        let else_if = If::new(Self::get_required_attr(TAG_IF, ATTR_COND, attr));
        self.add_executable_content(Box::new(else_if));

        let else_if_content_id = self.start_executable_content_region(true, TAG_ELSEIF);

        // Put together
        let else_if_ec = self.get_last_executable_content_entry_for_region(else_id);
        match get_opt_executable_content_as::<If>(else_if_ec) {
            Some(evc_if) => {
                evc_if.content = else_if_content_id;
            }
            None => {
                panic!(
                    "Internal Error: Executable Content missing in start_else_if in region #{}",
                    else_id
                );
            }
        }

        while if_id > 0 {
            // Find matching "if" level for the new "else if"
            let if_ec = self.get_last_executable_content_entry_for_region(if_id);
            match get_opt_executable_content_as::<If>(if_ec) {
                Some(evc_if) => {
                    if evc_if.else_content > 0 {
                        // Some higher "if". Go inside else-region.
                        if_id = evc_if.else_content;
                    } else {
                        // Match, set "else-region".
                        if_id = 0;
                        evc_if.else_content = else_id;
                    }
                }
                None => {
                    panic!("Internal Error: Executable Content missing in start_else_if");
                }
            }
        }
    }

    fn start_else(&mut self, _attr: &AttributeMap) {
        self.verify_parent_tag(TAG_ELSE, &[TAG_IF]);

        // Close parent <if> content region
        self.end_executable_content_region(TAG_IF);

        let mut if_id = self.current_executable_content;

        // Start new "else" region, replacing "If" region.
        let else_id = self.start_executable_content_region(true, TAG_IF);

        // Put together. Set deepest else
        while if_id > 0 {
            let if_ec = self.get_last_executable_content_entry_for_region(if_id);
            match get_opt_executable_content_as::<If>(if_ec) {
                Some(evc_if) => {
                    if evc_if.else_content > 0 {
                        if_id = evc_if.else_content;
                    } else {
                        if_id = 0;
                        evc_if.else_content = else_id;
                    }
                }
                None => {
                    panic!("Internal Error: Executable Content missing in start_else");
                }
            }
        }
    }

    fn start_send(&mut self, attr: &AttributeMap) {
        self.verify_parent_tag(
            TAG_SEND,
            &[
                TAG_TRANSITION,
                TAG_ON_EXIT,
                TAG_ON_ENTRY,
                TAG_IF,
                TAG_FOR_EACH,
            ],
        );

        let mut send_params = SendParameters::new();

        let event = attr.get(ATTR_EVENT);
        let eventexpr = attr.get(ATTR_EVENTEXPR);

        if let Some(event_value) = event {
            if eventexpr.is_some() {
                panic!(
                    "{}: attributes {} and {} must not occur both",
                    TAG_SEND, ATTR_EVENT, ATTR_EVENTEXPR
                );
            }
            send_params.event = Data::Source(event_value.clone());
        } else if let Some(eventexpr_value) = eventexpr {
            send_params.event_expr = Data::Source(eventexpr_value.clone());
        }

        let target = attr.get(ATTR_TARGET);
        let targetexpr = attr.get(ATTR_TARGETEXPR);
        if let Some(target_val) = target {
            if targetexpr.is_some() {
                panic!(
                    "{}: attributes {} and {} must not occur both",
                    TAG_SEND, ATTR_TARGET, ATTR_TARGETEXPR
                );
            }
            send_params.target = Data::Source(target_val.clone());
        } else if let Some(targetexpr_value) = targetexpr {
            send_params.target_expr = Data::Source(targetexpr_value.clone());
        }

        let type_attr = attr.get(ATTR_TYPE);
        let typeexpr = attr.get(ATTR_TYPEEXPR);
        if let Some(type_attr_value) = type_attr {
            if typeexpr.is_some() {
                panic!(
                    "{}: attributes {} and {} must not occur both",
                    TAG_SEND, ATTR_TYPE, ATTR_TYPEEXPR
                );
            }
            send_params.type_value = Data::Source(type_attr_value.clone());
        } else if let Some(typeexpr_value) = typeexpr {
            send_params.type_expr = Data::Source(typeexpr_value.clone());
        }

        let id = attr.get(ATTR_ID);
        let idlocation = attr.get(ATTR_IDLOCATION);
        if let Some(id_value) = id {
            if idlocation.is_some() {
                panic!(
                    "{}: attributes {} and {} must not occur both",
                    TAG_SEND, ATTR_ID, ATTR_IDLOCATION
                );
            }
            send_params.name.clone_from(id_value);
        } else if let Some(idlocation_value) = idlocation {
            send_params.name_location.clone_from(idlocation_value);
        }

        let delay_attr = attr.get(ATTR_DELAY);
        let delay_expr_attr = attr.get(ATTR_DELAYEXPR);

        if let Some(delay_expr_attr_value) = delay_expr_attr {
            if delay_attr.is_some() {
                panic!(
                    "{}: attributes {} and {} must not occur both",
                    TAG_SEND, ATTR_DELAY, ATTR_DELAYEXPR
                );
            }
            send_params.delay_expr = Data::Source(delay_expr_attr_value.clone());
        } else if delay_attr.is_some() {
            if (!delay_attr.unwrap().is_empty()) && type_attr.is_some() && type_attr.unwrap().eq(TARGET_INTERNAL) {
                panic!(
                    "{}: {} with {} {} is not possible",
                    TAG_SEND,
                    ATTR_DELAY,
                    ATTR_TARGET,
                    type_attr.unwrap()
                );
            }
            let delayms = parse_duration_to_milliseconds(delay_attr.unwrap());
            if delayms < 0 {
                panic!(
                    "{}: {} with illegal value '{}'",
                    TAG_SEND,
                    ATTR_DELAY,
                    delay_attr.unwrap()
                );
            } else {
                send_params.delay_ms = delayms as u64;
            }
        }

        if let Some(name_list_value) = attr.get(ATTR_NAMELIST) {
            self.parse_location_expressions(name_list_value, &mut send_params.name_list);
        }
        send_params
            .parent_state_name
            .clone_from(&self.get_current_state().name);
        self.add_executable_content(Box::new(send_params));
    }

    /// Reads the content until an end-tag is encountered.
    fn read_content(&mut self, tag: &str, reader: &mut XReader) -> String {
        let start = BytesStart::new(tag.to_string());
        let end = start.to_end().into_owned();

        let mut buf = Vec::new();
        let content = match reader.read_to_end_into(end.name(), &mut buf) {
            Ok(span) => {
                let r = self.content[(span.start as usize)..(span.end as usize)]
                    .trim()
                    .to_string();
                #[cfg(feature = "Debug_Reader")]
                debug!("{} content {} - {}: {}", tag, span.start, span.end, r);
                r
            }
            Err(e) => {
                panic!("XML invalid. {}", e);
            }
        };
        // Remove element from stack
        self.pop();

        content
    }

    fn start_content(&mut self, attr: &AttributeMap, reader: &mut XReader, has_content: bool) {
        self.verify_parent_tag(TAG_CONTENT, &[TAG_SEND, TAG_INVOKE, TAG_DONEDATA]);

        let parent_tag = self.get_parent_tag().to_string();
        let expr = attr.get(ATTR_EXPR);

        let content = if has_content {
            Some(self.read_content(TAG_CONTENT, reader))
        } else {
            None
        };

        // W3C:
        // A conformant SCXML document must not specify both the 'expr' attribute and child content.
        if expr.is_some() && content.is_some() {
            panic!(
                "{} shall have only {} or children, but not both.",
                TAG_CONTENT, ATTR_EXPR
            );
        }

        match parent_tag.as_str() {
            TAG_DONEDATA => {
                let state = self.get_current_state();
                match state.donedata.as_mut() {
                    Some(dd) => {
                        dd.content = Some(CommonContent {
                            content,
                            content_expr: expr.map(|x| x.to_string()),
                        });
                    }
                    None => {
                        panic!("Internal Error: donedata-Option not initialized")
                    }
                }
            }
            TAG_INVOKE => {
                let state = self.get_current_state();
                let invoke = state.invoke.last_mut();
                invoke.content = Some(CommonContent {
                    content,
                    content_expr: expr.map(|x| x.to_string()),
                });
            }
            TAG_SEND => {
                let ec_id = self.current_executable_content;
                let ec = self.get_last_executable_content_entry_for_region(ec_id);
                if ec.is_some() {
                    let send = get_safe_executable_content_as::<SendParameters>(ec.unwrap());
                    if expr.is_some() || content.is_some() {
                        send.content = Some(CommonContent {
                            content_expr: Option::map(expr, |v| v.clone()),
                            content,
                        });
                    }
                }
            }
            _ => {
                panic!(
                    "Internal Error: invalid parent-tag <{}> in start_content",
                    parent_tag
                )
            }
        }
    }

    fn start_param(&mut self, attr: &AttributeMap) {
        self.verify_parent_tag(TAG_PARAM, &[TAG_SEND, TAG_INVOKE, TAG_DONEDATA]);

        let parent_tag = self.get_parent_tag().to_string();

        let mut param = Parameter::new();

        param
            .name
            .clone_from(Self::get_required_attr(TAG_PARAM, ATTR_NAME, attr));
        let expr = attr.get(ATTR_EXPR);
        if let Some(expr_value) = expr {
            param.expr.clone_from(expr_value);
        }
        let location = attr.get(ATTR_LOCATION);
        if let Some(location_value) = location {
            if expr.is_some() {
                panic!(
                    "{} shall have only {} or {}, but not both.",
                    TAG_PARAM, ATTR_LOCATION, ATTR_EXPR
                );
            }
            param.location = location_value.to_string();
        }

        match parent_tag.as_str() {
            TAG_SEND => {
                let ec_id = self.current_executable_content;
                let ec = self.get_last_executable_content_entry_for_region(ec_id);
                let send = get_safe_executable_content_as::<SendParameters>(ec.unwrap());
                push_param(&mut send.params, param);
            }
            TAG_INVOKE => {
                let state = self.get_current_state();
                push_param(&mut state.invoke.last_mut().params, param);
            }
            TAG_DONEDATA => {
                let state = self.get_current_state();
                match state.donedata.as_mut() {
                    Some(dd) => {
                        push_param(&mut dd.params, param);
                    }
                    None => {
                        panic!("Internal Error: donedata-Option not initialized")
                    }
                }
            }
            _ => {
                panic!(
                    "Internal Error: invalid parent-tag <{}> in start_param",
                    parent_tag
                )
            }
        }
    }

    fn start_log(&mut self, attr: &AttributeMap) {
        self.verify_parent_tag(
            TAG_LOG,
            &[
                TAG_TRANSITION,
                TAG_ON_EXIT,
                TAG_ON_ENTRY,
                TAG_IF,
                TAG_FOR_EACH,
                TAG_FINALIZE,
            ],
        );
        let label = attr.get(ATTR_LABEL);
        let expr = attr.get(ATTR_EXPR);
        if let Some(exprs) = expr {
            self.add_executable_content(Box::new(Log::new(&label, exprs.as_str())));
        }
    }

    fn start_assign(&mut self, attr: &AttributeMap, reader: &mut XReader, has_content: bool) {
        self.verify_parent_tag(
            TAG_ASSIGN,
            &[
                TAG_TRANSITION,
                TAG_ON_EXIT,
                TAG_ON_ENTRY,
                TAG_IF,
                TAG_FOR_EACH,
                TAG_FINALIZE,
            ],
        );

        let mut assign = Assign::new();
        assign
            .location
            .clone_from(Self::get_required_attr(TAG_ASSIGN, ATTR_LOCATION, attr));

        let expr = attr.get(ATTR_EXPR);
        if let Some(expr_value) = expr {
            assign.expr.clone_from(expr_value);
        }

        let assign_text = if has_content {
            format!(
                "\"{}\"",
                self.read_content(TAG_ASSIGN, reader)
                    .replace("\"", "\\\"")
                    .replace("\n", " ")
            )
        } else {
            String::new()
        };

        let assign_src = assign_text.trim();

        if !assign_src.is_empty() {
            if !assign.expr.is_empty() {
                panic!("<assign> with 'expr' attribute shall not have content.")
            }
            assign.expr = assign_src.to_string();
        }

        self.add_executable_content(Box::new(assign));
    }

    fn start_raise(&mut self, attr: &AttributeMap) {
        self.verify_parent_tag(
            TAG_RAISE,
            &[
                TAG_TRANSITION,
                TAG_ON_EXIT,
                TAG_ON_ENTRY,
                TAG_IF,
                TAG_FOR_EACH,
            ],
        );

        let mut raise = Raise::new();
        raise
            .event
            .clone_from(Self::get_required_attr(TAG_RAISE, ATTR_EVENT, attr));

        self.add_executable_content(Box::new(raise));
    }

    fn start_scxml(&mut self, attr: &AttributeMap) {
        if self.in_scxml {
            panic!("Only one <{}> allowed", TAG_SCXML);
        }
        self.in_scxml = true;
        match attr.get(ATTR_NAME) {
            Some(n) => {
                self.fsm.name.clone_from(n);
            }
            None => {
                // @TODO: Filename?
            }
        }
        let datamodel = attr.get(ATTR_DATAMODEL);
        if datamodel.is_some() {
            #[cfg(feature = "Debug_Reader")]

            debug!(" scxml.datamodel = {}", datamodel.unwrap());
            self.fsm.datamodel = datamodel.unwrap().to_string();
        }

        let binding = attr.get(ATTR_BINDING);
        if binding.is_some() {
            match BindingType::from_str(binding.unwrap()) {
                Ok(t) => {
                    self.fsm.binding = t;
                }
                Err(_e) => {
                    panic!("{}: unsupported value {}", ATTR_BINDING, binding.unwrap())
                }
            }
        }
        let version = attr.get(TAG_VERSION);
        if let Some(version_value) = version {
            self.fsm.version.clone_from(version_value);
            #[cfg(feature = "Debug_Reader")]
            debug!(" scxml.version = {}", version_value);
        }
        self.fsm.pseudo_root = self.get_or_create_state_with_attributes(attr, false, 0);
        self.current.current_state = self.fsm.pseudo_root;
    }

    fn end_scxml(&mut self) {
        self.set_default_initial(self.fsm.pseudo_root);
    }

    fn set_default_initial(&mut self, id: StateId) {
        let state = self.get_state_by_id_mut(id);
        if state.initial == 0 {
            //  W3C: If not specified, the default initial state is the first child state in document order.
            match state.states.first() {
                None => {
                    // No states at all
                }
                Some(first_state) => {
                    let mut t = Transition::new();
                    state.initial = t.id;
                    t.source = state.id;
                    t.target.push(*first_state);
                    self.fsm.transitions.insert(t.id, t);
                }
            }
        }
    }

    fn end_state(&mut self) {
        //  W3C: If not specified, the default initial state is the first child state in document order.
        self.set_default_initial(self.current.current_state);
    }

    fn start_element(&mut self, reader: &mut XReader, e: &BytesStart, has_content: bool) {
        let n = e.local_name();
        let name = str::from_utf8(n.as_ref()).unwrap();
        self.push(name);

        #[cfg(feature = "Debug_Reader")]
        debug!("Start Element {}", name);

        let attr = &decode_attributes(reader, &mut e.attributes());

        match name {
            TAG_INCLUDE => {
                self.include(attr);
            }
            TAG_SCXML => {
                self.start_scxml(attr);
            }
            TAG_DATAMODEL => {
                self.start_datamodel();
            }
            TAG_DATA => {
                self.start_data(attr, reader, has_content);
            }
            TAG_STATE => {
                self.start_state(attr);
            }
            TAG_PARALLEL => {
                self.start_parallel(attr);
            }
            TAG_FINAL => {
                self.start_final(attr);
            }
            TAG_DONEDATA => {
                self.start_donedata();
            }
            TAG_HISTORY => {
                self.start_history(attr);
            }
            TAG_INITIAL => {
                self.start_initial();
            }
            TAG_INVOKE => {
                self.start_invoke(attr);
            }
            TAG_TRANSITION => {
                self.start_transition(attr);
            }
            TAG_FINALIZE => {
                self.start_finalize(attr);
            }
            TAG_ON_ENTRY => {
                self.start_on_entry(attr);
            }
            TAG_ON_EXIT => {
                self.start_on_exit(attr);
            }
            TAG_SCRIPT => {
                self.start_script(attr, reader, has_content);
            }
            TAG_RAISE => {
                self.start_raise(attr);
            }
            TAG_SEND => {
                self.start_send(attr);
            }
            TAG_PARAM => {
                self.start_param(attr);
            }
            TAG_CONTENT => {
                self.start_content(attr, reader, has_content);
            }
            TAG_LOG => {
                self.start_log(attr);
            }
            TAG_ASSIGN => {
                self.start_assign(attr, reader, has_content);
            }
            TAG_FOR_EACH => {
                self.start_for_each(attr);
            }
            TAG_CANCEL => {
                self.start_cancel(attr);
            }
            TAG_IF => {
                self.start_if(attr);
            }
            TAG_ELSE => {
                self.start_else(attr);
            }
            TAG_ELSEIF => {
                self.start_else_if(attr);
            }
            _ => {
                #[cfg(feature = "Debug_Reader")]
                debug!("Ignored tag {}", name)
            }
        }
    }

    /// Try to resolve the file name relative to the current file or include paths.
    fn get_resolved_path(&self, ps: &str) -> Result<PathBuf, String> {
        let mut ps = ps.to_string();
        while ps.starts_with('\\') || ps.starts_with('/') {
            ps.remove(0);
        }
        let src = Path::new(&ps);

        let to_current = {
            let parent = self.file.parent();
            match parent {
                Some(parent_path) => {
                    let pp = parent_path.join(src);
                    pp.to_owned()
                }
                None => src.to_owned(),
            }
        };
        if to_current.exists() {
            return Ok(to_current);
        } else {
            for ip in &self.include_paths {
                let rp = ip.join(src);
                if rp.exists() {
                    return Ok(rp);
                }
            }
        }
        Err(format!("Can not resolve {:?}", ps))
    }

    /// Handle a XInclude include element.
    /// See https://www.w3.org/TR/xinclude/
    /// Only parse="text" and "href" with a relative path are supported, also no "xpointer" etc.
    fn include(&mut self, attr: &AttributeMap) {
        let href = Self::get_required_attr(TAG_INCLUDE, TAG_HREF, attr);
        let parse = attr.get(ATTR_PARSE);
        if parse.is_none() || parse.unwrap().ne("text") {
            panic!("{}: only {}='text' is supported", TAG_INCLUDE, ATTR_PARSE)
        }
        let xpointer = attr.get(ATTR_XPOINTER);
        if xpointer.is_some() {
            panic!("{}: {} is not supported", TAG_INCLUDE, ATTR_XPOINTER)
        }

        // remove "include" from parent-stack as long as we read the content.
        self.pop();

        match self.get_resolved_path(href.as_str()) {
            Ok(src) => {
                let org_file = mem::take(&mut self.file);
                let org_content = mem::take(&mut self.content);
                let rs = self.process_file(&src);
                if rs.is_err() {
                    panic!("Failed to read {:?}. {}", src, rs.err().unwrap());
                }
                self.file = org_file;
                self.content = org_content;
            }
            Err(err) => {
                panic!("Can resolve path {}: {}", href, err);
            }
        }

        self.push(TAG_INCLUDE);
    }

    /// Called from SAX handler if some end-tag was read.
    fn end_element(&mut self, name: &str) {
        if !self.current.current_tag.eq(name) {
            panic!(
                "Illegal end-tag {:?}, expected {:?}",
                &name, &self.current.current_tag
            );
        }
        #[cfg(feature = "Debug_Reader")]

        debug!("End Element {}", name);
        match name {
            TAG_SCXML => {
                self.end_scxml();
            }
            TAG_IF => {
                self.end_if();
            }
            TAG_TRANSITION => {
                self.end_transition();
            }
            TAG_ON_EXIT => {
                self.end_on_exit();
            }
            TAG_ON_ENTRY => {
                self.end_on_entry();
            }
            TAG_FOR_EACH => {
                self.end_for_each();
            }
            TAG_FINALIZE => {
                self.end_finalize();
            }
            TAG_STATE => {
                self.end_state();
            }
            _ => {}
        }
        self.pop();
    }
}

/**
 * Decodes attributes into a hash-map
 */
fn decode_attributes(reader: &XReader, attr: &mut Attributes) -> AttributeMap {
    attr.map(|attr_result| match attr_result {
        Ok(a) => {
            let key = reader.decoder().decode(a.key.as_ref());
            if key.is_err() {
                panic!(
                    "unable to read attribute name {:?}, utf8 error {:?}",
                    &a,
                    key.err()
                );
            }
            let value = a.decode_and_unescape_value(reader.decoder());
            if value.is_err() {
                panic!(
                    "unable to read attribute value  {:?}, utf8 error {:?}",
                    &a,
                    value.err()
                );
            }
            (key.unwrap().to_string(), value.unwrap().to_string())
        }
        Err(err) => {
            panic!("unable to read key in DefaultSettings, err = {:?}", err);
        }
    })
    .collect()
}

/// Read and parse the FSM from an XML file
pub fn parse_from_xml_file(file: &Path, include_paths: &[PathBuf]) -> Result<Box<Fsm>, String> {
    let mut rs = ReaderState::new();
    rs.include_paths = Vec::from(include_paths);
    let r = rs.process_file(file);
    match r {
        Ok(_m) => Ok(rs.fsm),
        Err(e) => Err(e),
    }
}

/// Read and parse the FSM from an URI
pub fn parse_from_uri(uri: String, include_paths: &[PathBuf]) -> Result<Box<Fsm>, String> {
    let start = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let mut rs = ReaderState::new();
    rs.include_paths = Vec::from(include_paths);
    match rs.read_from_uri(&uri) {
        Ok(source) => {
            rs.content = source;
            let r = rs.process();
            match r {
                Ok(_m) => {
                    let end = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
                    info!(
                        "'{}' (XML) loaded in {}ms",
                        rs.fsm.name,
                        end.as_millis() - start.as_millis()
                    );
                    Ok(rs.fsm)
                }
                Err(e) => Err(e),
            }
        }
        Err(err) => Err(err),
    }
}

/// Reads the FSM from a XML String
pub fn parse_from_xml(xml: String) -> Result<Box<Fsm>, String> {
    let mut rs = ReaderState::new();
    rs.content = xml;
    let r = rs.process();
    match r {
        Ok(_m) => Ok(rs.fsm),
        Err(e) => Err(e),
    }
}

/// Reads the FSM from a XML String
pub fn parse_from_xml_with_includes(xml: String, include_paths: &[PathBuf]) -> Result<Box<Fsm>, String> {
    let mut rs = ReaderState::new();
    rs.include_paths = Vec::from(include_paths);
    rs.content = xml;
    let r = rs.process();
    match r {
        Ok(_m) => Ok(rs.fsm),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use log::debug;

    #[test]
    #[should_panic]
    fn initial_attribute_should_panic() {
        let _r = crate::scxml_reader::parse_from_xml(
            "<scxml initial='Main'><state id='Main' initial='A'>\
    <initial><transition></transition></initial></state></scxml>"
                .to_string(),
        );
    }

    #[test]
    #[should_panic]
    fn script_with_src_and_content_should_panic() {
        let _r = crate::scxml_reader::parse_from_xml("<scxml initial='Main'><state id='Main'>\
    <initial><transition><script src='xml/example/script.js'>println();</script></transition></initial></state></scxml>".to_string());
    }

    #[test]
    fn script_with_src_should_load_file() {
        let r = crate::scxml_reader::parse_from_xml(
            "<scxml initial='Main'><state id='Main'>\
    <transition><script src='xml/example/script.js'></script></transition></state></scxml>"
                .to_string(),
        );
        assert!(r.is_ok());

        let fsm = r.unwrap();

        let mut b = false;
        for s in &fsm.states {
            debug!("State {}", s.name);
            for tid in s.transitions.iterator() {
                let tr = fsm.transitions.get(tid).unwrap();
                debug!(" Transition #{} content {}", tr.id, tr.content);
                if tr.content != 0 {
                    debug!(" -> {:?}", fsm.executableContent.get(&tr.content).unwrap());
                    b = true;
                }
            }
        }
        assert!(b);
    }

    #[test]
    fn initial_attribute() {
        let _r = crate::scxml_reader::parse_from_xml(
            "<scxml initial='Main'><state id='Main' initial='A'></state></scxml>".to_string(),
        );
    }

    #[test]
    fn wrong_end_tag_should_panic() {
        let r = crate::scxml_reader::parse_from_xml(
            "<scxml initial='Main'><state id='Main' initial='A'></parallel></scxml>".to_string(),
        );
        assert!(r.is_err(), "Shall result in error");
    }

    #[test]
    #[should_panic]
    fn wrong_parse_in_xinclude_should_panic() {
        let _r = crate::scxml_reader::parse_from_xml(
            "<scxml><state><include href='xml/example/Test2Sub1.xml' parse='xml'/></state></scxml>".to_string(),
        );
    }

    #[test]
    #[should_panic]
    fn none_parse_in_xinclude_should_panic() {
        let _r = crate::scxml_reader::parse_from_xml(
            "<scxml><state><include href='xml/example/Test2Sub1.xml'/></state></scxml>".to_string(),
        );
    }

    #[test]
    #[should_panic]
    fn xpointer_in_xinclude_should_panic() {
        let _r = crate::scxml_reader::parse_from_xml(
            "<scxml><state><include href='xml/example/Test2Sub1.xml' parse='text' xpointer='#123'/></state></scxml>"
                .to_string(),
        );
    }

    #[test]
    fn xinclude_should_read() {
        let _r = crate::scxml_reader::parse_from_xml(
            "<scxml><state><include href='xml/example/Test2Sub1.xml' parse='text'/></state></scxml>".to_string(),
        );
    }

    #[test]
    #[should_panic]
    fn wrong_transition_type_should_panic() {
        let _r = crate::scxml_reader::parse_from_xml(
            "<scxml><state><transition type='bla'></transition></state></scxml>".to_string(),
        );
    }

    #[test]
    fn transition_type_internal() {
        let _r = crate::scxml_reader::parse_from_xml(
            "<scxml><state><transition type='internal'></transition></state></scxml>".to_string(),
        );
    }

    #[test]
    fn transition_type_external() {
        let _r = crate::scxml_reader::parse_from_xml(
            "<scxml><state><transition type='external'></transition></state></scxml>".to_string(),
        );
    }

    #[test]
    #[should_panic]
    fn assign_with_expr_and_content_shall_panic() {
        let _r = crate::scxml_reader::parse_from_xml(
            "<scxml><state><transition><assign location='x' expr='123'>123</assign></transition></state></scxml>"
                .to_string(),
        );
    }

    #[test]
    fn assign_without_expr_and_content() {
        let _r = crate::scxml_reader::parse_from_xml(
            "<scxml><state><transition><assign location='x'>123</assign></transition></state></scxml>".to_string(),
        );
    }
}
