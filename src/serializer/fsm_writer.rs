//! Module to write a persistent binary version of a Fsm.\
//! The format is independent of the platform byte-order

use crate::datamodel::DataArc;
#[cfg(feature = "Debug_Serializer")]
use log::debug;
use std::collections::HashMap;
use std::io::Write;

use crate::executable_content;
use crate::executable_content::{
    Assign, Cancel, ExecutableContent, Expression, ForEach, If, Log, Raise, Script, SendParameters,
};
use crate::fsm::{
    CommonContent, DocumentId, DoneData, ExecutableContentId, Fsm, Invoke, Parameter, State, StateId, Transition,
    TransitionId,
};
use crate::serializer::default_protocol_definitions::{
    FSM_PROTOCOL_FLAG_DATA, FSM_PROTOCOL_FLAG_DONE_DATA, FSM_PROTOCOL_FLAG_HISTORY, FSM_PROTOCOL_FLAG_INVOKE,
    FSM_PROTOCOL_FLAG_IS_FINAL, FSM_PROTOCOL_FLAG_IS_PARALLEL, FSM_PROTOCOL_FLAG_ON_ENTRY, FSM_PROTOCOL_FLAG_ON_EXIT,
    FSM_PROTOCOL_FLAG_STATES,
};
use crate::serializer::protocol_writer::ProtocolWriter;

pub const FSM_PROTOCOL_WRITER_VERSION: &str = "fsmW1.1";

fn get_executable_content_as<T: 'static>(ec: &dyn crate::executable_content::ExecutableContent) -> &T {
    let va = ec.as_any();
    va.downcast_ref::<T>()
        .unwrap_or_else(|| panic!("Failed to cast executable content"))
}

pub struct FsmWriter<'a, W>
where
    W: Write + 'a,
{
    pub writer: Box<dyn ProtocolWriter<W> + 'a>,
}

impl<'a, W> FsmWriter<'a, W>
where
    W: Write + 'a,
{
    pub fn get_writer(&self) -> &W {
        return self.writer.get_writer();
    }

    pub fn new(writer: Box<(dyn ProtocolWriter<W> + 'a)>) -> FsmWriter<'a, W> {
        FsmWriter { writer }
    }

    pub fn write(&mut self, fsm: &Fsm) {
        self.writer.write_str(FSM_PROTOCOL_WRITER_VERSION);
        self.writer.write_str(fsm.name.as_str());
        self.writer.write_str(&fsm.datamodel);
        self.writer.write_u8(fsm.binding.ordinal());
        self.write_state_id(fsm.pseudo_root);
        self.write_executable_content_id(fsm.script);

        self.writer.write_usize(fsm.states.len());
        for state in &fsm.states {
            self.write_state(state);
        }

        self.writer.write_usize(fsm.transitions.len());
        for transition in fsm.transitions.values() {
            self.write_transition(transition);
        }

        self.writer.write_usize(fsm.executableContent.len());
        for (content_id, content) in &fsm.executableContent {
            self.write_executable_content_id(*content_id);
            self.writer.write_usize(content.len());
            for executable_content in content {
                self.write_executable_content(executable_content.as_ref());
            }
        }
    }

    pub fn close(&mut self) {
        self.writer.close();
    }

    pub fn write_state_id(&mut self, value: StateId) {
        self.writer.write_uint(value as u64);
    }

    pub fn write_doc_id(&mut self, value: DocumentId) {
        self.writer.write_uint(value as u64);
    }

    pub fn write_transition_id(&mut self, value: TransitionId) {
        self.writer.write_uint(value as u64);
    }

    pub fn write_executable_content_id(&mut self, value: ExecutableContentId) {
        self.writer.write_uint(value as u64);
    }

    pub fn write_data_map(&mut self, value: &HashMap<String, DataArc>) {
        self.writer.write_usize(value.len());
        for (key, data) in value {
            self.writer.write_str(key.as_str());
            self.writer.write_data_arc(data);
        }
    }

    pub fn write_common_content(&mut self, value: &CommonContent) {
        self.writer.write_option_string(&value.content);
        self.writer.write_option_string(&value.content_expr);
    }

    pub fn write_parameter(&mut self, value: &Parameter) {
        self.writer.write_str(value.name.as_str());
        self.writer.write_str(value.expr.as_str());
        self.writer.write_str(value.location.as_str());
    }

    pub fn write_done_data(&mut self, value: &DoneData) {
        self.writer.write_boolean(value.content.is_some());
        if value.content.is_some() {
            self.write_common_content(value.content.as_ref().unwrap());
        }
        self.write_parameters(&value.params);
    }

    pub fn write_parameters(&mut self, parameters: &Option<Vec<Parameter>>) {
        if parameters.is_some() {
            let params = parameters.as_ref().unwrap();
            self.writer.write_usize(params.len());
            for p in params {
                self.write_parameter(p);
            }
        } else {
            self.writer.write_usize(0usize);
        }
    }

    pub fn write_string_list(&mut self, strings: &Vec<String>) {
        self.writer.write_usize(strings.len());
        for s in strings {
            self.writer.write_str(s);
        }
    }

    pub fn write_invoke(&mut self, invoke: &Invoke) {
        self.writer.write_str(&invoke.invoke_id);
        if invoke.invoke_id.is_empty() {
            self.writer.write_str(&invoke.parent_state_name);
        }
        self.write_doc_id(invoke.doc_id);
        self.writer.write_data(&invoke.src_expr);
        self.writer.write_data(&invoke.src);
        self.writer.write_data(&invoke.type_expr);
        self.writer.write_data(&invoke.type_name);
        self.writer.write_str(&invoke.external_id_location);
        self.writer.write_boolean(invoke.autoforward);
        self.write_executable_content_id(invoke.finalize);

        if let Some(cc) = &invoke.content {
            self.writer.write_boolean(true);
            self.write_common_content(cc);
        } else {
            self.writer.write_boolean(false);
        }

        self.write_parameters(&invoke.params);
        self.write_string_list(&invoke.name_list);

        // parent_state_name is not written, the reader needs to
        // restore it from the current state.
    }

    pub fn write_transition(&mut self, transition: &Transition) {
        #[cfg(feature = "Debug_Serializer")]
        debug!(">>Transition #{}", transition.id);
        self.write_transition_id(transition.id);
        self.write_doc_id(transition.doc_id);
        self.write_state_id(transition.source);
        self.writer.write_usize(transition.target.len());
        for t in &transition.target {
            self.write_state_id(*t);
        }
        self.writer.write_usize(transition.events.len());
        for e in &transition.events {
            self.writer.write_str(e);
        }
        self.writer.write_u8(
            transition.transition_type.ordinal() // 0 - 1
            | if transition.wildcard {2u8} else {0u8}
            | if transition.cond.is_empty() {0u8} else {4u8}
            | if transition.content != 0 {8u8} else {0u8},
        );

        if !transition.cond.is_empty() {
            self.writer.write_data(&transition.cond);
        }
        if transition.content != 0 {
            self.write_executable_content_id(transition.content);
        }

        #[cfg(feature = "Debug_Serializer")]
        debug!("<<Transition");
    }

    pub fn write_state(&mut self, state: &State) {
        #[cfg(feature = "Debug_Serializer")]
        debug!(">>State {}", state.name);

        self.write_state_id(state.id);
        self.write_doc_id(state.doc_id);
        self.writer.write_str(state.name.as_str());

        let flags = state.history_type.ordinal() as u16 // 0 - 2
                | if state.onentry.is_empty() {0} else {FSM_PROTOCOL_FLAG_ON_ENTRY}
                | if state.onexit.is_empty() {0} else {FSM_PROTOCOL_FLAG_ON_EXIT}
                | if !state.states.is_empty() {FSM_PROTOCOL_FLAG_STATES} else {0}
                | if state.is_final {FSM_PROTOCOL_FLAG_IS_FINAL} else {0}
                | if state.is_parallel {FSM_PROTOCOL_FLAG_IS_PARALLEL} else {0}
                | if state.donedata.is_some() {FSM_PROTOCOL_FLAG_DONE_DATA} else {0}
                | if state.invoke.size()>0 {FSM_PROTOCOL_FLAG_INVOKE} else {0}
                | if !state.data.is_empty()  {FSM_PROTOCOL_FLAG_DATA} else {0}
                | if state.history.size() > 0 {FSM_PROTOCOL_FLAG_HISTORY} else {0};
        self.writer.write_uint(flags as u64);

        if !state.states.is_empty() {
            self.write_transition_id(state.initial);
            self.writer.write_usize(state.states.len());
            for state_id in &state.states {
                self.write_state_id(*state_id);
            }
        }

        if !state.onentry.is_empty() {
            self.writer.write_usize(state.onentry.len());
            for ec in &state.onentry {
                self.write_executable_content_id(*ec);
            }
        }
        if !state.onexit.is_empty() {
            self.writer.write_usize(state.onexit.len());
            for ec in &state.onexit {
                self.write_executable_content_id(*ec);
            }
        }

        self.writer.write_usize(state.transitions.size());
        for transition_id in state.transitions.iterator() {
            self.write_transition_id(*transition_id);
        }

        if state.invoke.size() > 0 {
            self.writer.write_usize(state.invoke.size());
            for invoke in state.invoke.iterator() {
                self.write_invoke(invoke);
            }
        }

        if state.history.size() > 0 {
            self.writer.write_usize(state.history.size());
            for history in state.history.iterator() {
                self.write_state_id(*history);
            }
        }

        if !state.data.is_empty() {
            self.write_data_map(&state.data);
        }

        self.write_state_id(state.parent);

        if state.donedata.is_some() {
            self.write_done_data(state.donedata.as_ref().unwrap())
        }

        #[cfg(feature = "Debug_Serializer")]
        debug!("<<State");
    }

    pub fn write_executable_content(&mut self, executable_content: &dyn ExecutableContent) {
        let ec_type = executable_content.get_type();
        self.writer.write_u8(ec_type);

        match ec_type {
            executable_content::TYPE_IF => {
                self.write_executable_content_if(get_executable_content_as::<If>(executable_content))
            }
            executable_content::TYPE_EXPRESSION => {
                self.write_executable_content_expression(get_executable_content_as::<Expression>(executable_content))
            }
            executable_content::TYPE_SCRIPT => {
                self.write_executable_content_script(get_executable_content_as::<Script>(executable_content))
            }
            executable_content::TYPE_LOG => {
                self.write_executable_content_log(get_executable_content_as::<Log>(executable_content))
            }
            executable_content::TYPE_FOREACH => {
                self.write_executable_content_for_each(get_executable_content_as::<ForEach>(executable_content))
            }
            executable_content::TYPE_SEND => self.write_executable_content_send(get_executable_content_as::<
                SendParameters,
            >(executable_content)),
            executable_content::TYPE_RAISE => {
                self.write_executable_content_raise(get_executable_content_as::<Raise>(executable_content))
            }
            executable_content::TYPE_CANCEL => {
                self.write_executable_content_cancel(get_executable_content_as::<Cancel>(executable_content))
            }
            executable_content::TYPE_ASSIGN => {
                self.write_executable_content_assign(get_executable_content_as::<Assign>(executable_content))
            }
            ut => {
                panic!("Unknown Executable Content: {}", ut)
            }
        }
    }

    pub fn write_executable_content_if(&mut self, executable_content_if: &If) {
        self.writer.write_data(&executable_content_if.condition);
        self.write_executable_content_id(executable_content_if.content);
        self.write_executable_content_id(executable_content_if.else_content);
    }
    pub fn write_executable_content_expression(&mut self, executable_content_expression: &Expression) {
        self.writer
            .write_data(&executable_content_expression.content);
    }

    pub fn write_executable_content_script(&mut self, executable_content_script: &Script) {
        self.writer
            .write_usize(executable_content_script.content.len());
        for ec_id in &executable_content_script.content {
            self.write_executable_content_id(*ec_id);
        }
    }

    pub fn write_executable_content_log(&mut self, executable_content_log: &Log) {
        self.writer.write_str(&executable_content_log.label);
        self.writer.write_data(&executable_content_log.expression);
    }

    pub fn write_executable_content_for_each(&mut self, executable_content_for_each: &ForEach) {
        self.write_executable_content_id(executable_content_for_each.content);
        self.writer.write_str(&executable_content_for_each.index);
        self.writer.write_str(&executable_content_for_each.array);
        self.writer.write_str(&executable_content_for_each.item);
    }
    pub fn write_executable_content_send(&mut self, executable_content_send: &SendParameters) {
        self.writer.write_str(&executable_content_send.name);
        self.writer.write_data(&executable_content_send.target);
        self.writer.write_data(&executable_content_send.target_expr);

        if let Some(ct) = &executable_content_send.content {
            self.writer.write_boolean(true);
            self.write_common_content(ct);
        } else {
            self.writer.write_boolean(false);
        }

        self.write_string_list(&executable_content_send.name_list);
        self.writer
            .write_str(&executable_content_send.name_location);
        self.write_parameters(&executable_content_send.params);

        self.writer.write_data(&executable_content_send.event);
        self.writer.write_data(&executable_content_send.event_expr);

        self.writer.write_data(&executable_content_send.type_value);
        self.writer.write_data(&executable_content_send.type_expr);

        self.writer.write_uint(executable_content_send.delay_ms);
        self.writer.write_data(&executable_content_send.delay_expr);
    }

    pub fn write_executable_content_raise(&mut self, executable_content_raise: &Raise) {
        self.writer.write_str(&executable_content_raise.event);
    }
    pub fn write_executable_content_cancel(&mut self, executable_content_cancel: &Cancel) {
        self.writer.write_str(&executable_content_cancel.send_id);
        self.writer
            .write_data(&executable_content_cancel.send_id_expr);
    }

    pub fn write_executable_content_assign(&mut self, executable_content_assign: &Assign) {
        self.writer.write_str(&executable_content_assign.expr);
        self.writer.write_str(&executable_content_assign.location);
    }
}
