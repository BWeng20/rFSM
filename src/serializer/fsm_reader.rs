//! Module to write a persistent binary version of a Fsm.\
//! The format is independent of the platform byte-order

#[cfg(feature = "Debug_Serializer")]
use log::debug;

use std::io::Read;

use crate::datamodel::{Data, DataStore};
use crate::executable_content;
use crate::executable_content::{ExecutableContent, Log};
use crate::fsm::{
    BindingType, CommonContent, DocumentId, DoneData, ExecutableContentId, Fsm, HistoryType,
    Invoke, Parameter, State, StateId, Transition, TransitionId, TransitionType,
};
use crate::serializer::default_protocol_definitions::{
    FSM_PROTOCOL_FLAG_DATA, FSM_PROTOCOL_FLAG_DONE_DATA, FSM_PROTOCOL_FLAG_HISTORY,
    FSM_PROTOCOL_FLAG_HISTORY_TYPE_MASK, FSM_PROTOCOL_FLAG_INVOKE, FSM_PROTOCOL_FLAG_IS_FINAL,
    FSM_PROTOCOL_FLAG_IS_PARALLEL, FSM_PROTOCOL_FLAG_ON_ENTRY, FSM_PROTOCOL_FLAG_ON_EXIT,
    FSM_PROTOCOL_FLAG_STATES,
};
use crate::serializer::fsm_writer::FSM_PROTOCOL_WRITER_VERSION;
use crate::serializer::protocol_reader::ProtocolReader;

/// The reader version, must natch the corresponding writer version
pub const FSM_READER_VERSION: &str = "fsmW1.0";

pub struct FsmReader<'a, R>
where
    R: Read + 'a,
{
    reader: Box<dyn ProtocolReader<R> + 'a>,
}

impl<'a, R> FsmReader<'a, R>
where
    R: Read + 'a,
{
    pub fn new(reader: Box<dyn ProtocolReader<R> + 'a>) -> FsmReader<'a, R> {
        FsmReader { reader }
    }

    pub fn read(&mut self) -> Result<Box<Fsm>, String> {
        let mut fsm = Fsm::new();
        let version = self.reader.read_string();
        if version.as_str() == FSM_PROTOCOL_WRITER_VERSION {
            fsm.name = self.reader.read_string();
            fsm.datamodel = self.reader.read_string();
            fsm.binding = BindingType::from_ordinal(self.reader.read_u8());
            fsm.pseudo_root = self.read_state_id();
            fsm.script = self.read_executable_content_id();

            let states_len = self.reader.read_usize();
            for _idx in 0..states_len {
                let mut state = State::new("");
                self.read_state(&mut state);
                fsm.states.push(state);
            }

            let transitions_len = self.reader.read_usize();
            for _idx in 0..transitions_len {
                let transition = self.read_transition();
                fsm.transitions.insert(transition.id, transition);
            }

            let executable_content_len = self.reader.read_usize();
            for _idx in 0..executable_content_len {
                let content_id = self.read_executable_content_id();
                let content_len = self.reader.read_usize();
                let mut content = Vec::new();
                for _idx2 in 0..content_len {
                    content.push(self.read_executable_content());
                }
                fsm.executableContent.insert(content_id, content);
            }

            Ok(Box::new(fsm))
        } else if self.reader.has_error() {
            Err("Can't read".to_string())
        } else {
            Err(format!(
                "Version mismatch: '{}' is not '{}' as expected",
                version, FSM_PROTOCOL_WRITER_VERSION
            ))
        }
    }

    pub fn close(&mut self) {
        self.reader.close();
    }

    pub fn read_state_id(&mut self) -> StateId {
        self.reader.read_uint() as StateId
    }

    pub fn read_doc_id(&mut self) -> DocumentId {
        self.reader.read_uint() as DocumentId
    }

    pub fn read_transition_id(&mut self) -> TransitionId {
        self.reader.read_uint() as TransitionId
    }

    pub fn read_executable_content_id(&mut self) -> ExecutableContentId {
        self.reader.read_uint() as ExecutableContentId
    }

    pub fn read_data_store(&mut self, value: &mut DataStore) {
        value.values.clear();
        let len = self.reader.read_usize();
        for _i in 0..len {
            let key = self.reader.read_string();
            let data = Data {
                value: self.reader.read_option_string(),
            };
            value.values.insert(key, data);
        }
    }

    pub fn read_common_content(&mut self, value: &mut CommonContent) {
        value.content = self.reader.read_option_string();
        value.content_expr = self.reader.read_option_string();
    }

    pub fn read_parameter(&mut self, value: &mut Parameter) {
        value.name = self.reader.read_string();
        value.expr = self.reader.read_string();
        value.location = self.reader.read_string();
    }

    pub fn read_done_data(&mut self, value: &mut DoneData) {
        if self.reader.read_boolean() {
            let mut c = CommonContent::new();
            self.read_common_content(&mut c);
            let _ = value.content.insert(c);
        } else {
            value.content = None;
        }
        self.read_parameters(&mut value.params);
    }

    pub fn read_parameters(&mut self, parameters: &mut Option<Vec<Parameter>>) {
        let param_len = self.reader.read_usize();
        if param_len == 0 {
            parameters.take();
        } else {
            let mut pvec = Vec::new();
            for _pi in 0..param_len {
                let mut param = Parameter::new();
                self.read_parameter(&mut param);
                pvec.push(param);
            }
            let _ = parameters.insert(pvec);
        }
    }

    pub fn read_string_list(&mut self) -> Vec<String> {
        let len = self.reader.read_usize();
        let mut pv = Vec::new();
        for _idx in 0..len {
            pv.push(self.reader.read_string());
        }
        pv
    }

    pub fn read_invoke(&mut self, invoke: &mut Invoke) {
        invoke.invoke_id = self.reader.read_string();
        invoke.doc_id = self.read_doc_id();
        invoke.src_expr = self.reader.read_string();
        invoke.src = self.reader.read_string();
        invoke.type_expr = self.reader.read_string();
        invoke.type_name = self.reader.read_string();
        invoke.external_id_location = self.reader.read_string();
        invoke.autoforward = self.reader.read_boolean();
        invoke.finalize = self.read_executable_content_id();

        if self.reader.read_boolean() {
            let mut cc = CommonContent::new();
            self.read_common_content(&mut cc);
            invoke.content = Some(cc);
        } else {
            invoke.content = None;
        }
        self.read_parameters(&mut invoke.params);
        invoke.name_list = self.read_string_list();
    }

    pub fn read_transition(&mut self) -> Transition {
        #[cfg(feature = "Debug_Serializer")]
        debug!(">>Transition");

        let mut transition = Transition::new();

        transition.id = self.read_transition_id();
        transition.doc_id = self.read_doc_id();
        transition.source = self.read_state_id();

        let target_len = self.reader.read_usize();
        for _idx in 0..target_len {
            transition.target.push(self.read_state_id())
        }

        let events_len = self.reader.read_usize();
        for _idx in 0..events_len {
            transition.events.push(self.reader.read_string())
        }

        let flags = self.reader.read_u8();

        transition.transition_type = TransitionType::from_ordinal(flags & 1);
        transition.wildcard = (flags & 2) != 0;

        transition.cond = if (flags & 4) != 0 {
            Some(self.reader.read_string())
        } else {
            None
        };
        transition.content = if (flags & 8) != 0 {
            self.read_executable_content_id()
        } else {
            0
        };

        #[cfg(feature = "Debug_Serializer")]
        debug!("<<Transition");

        transition
    }

    pub fn read_state(&mut self, state: &mut State) {
        #[cfg(feature = "Debug_Serializer")]
        debug!(">>State");

        state.id = self.read_state_id();
        state.doc_id = self.read_doc_id();
        state.name = self.reader.read_string();

        let flags = self.reader.read_u16();

        state.history_type =
            HistoryType::from_ordinal((flags & FSM_PROTOCOL_FLAG_HISTORY_TYPE_MASK) as u8);
        state.is_parallel = (flags & FSM_PROTOCOL_FLAG_IS_PARALLEL) != 0;
        state.is_final = (flags & FSM_PROTOCOL_FLAG_IS_FINAL) != 0;

        if (flags & FSM_PROTOCOL_FLAG_STATES) != 0 {
            state.initial = self.read_transition_id();
            let states_len = self.reader.read_usize();
            for _si in 0..states_len {
                state.states.push(self.read_state_id());
            }
        }

        if (flags & FSM_PROTOCOL_FLAG_ON_ENTRY) != 0 {
            let onentry_len = self.reader.read_usize();
            for _si in 0..onentry_len {
                state.onentry.push(self.read_executable_content_id());
            }
        }
        if (flags & FSM_PROTOCOL_FLAG_ON_EXIT) != 0 {
            let onexit_len = self.reader.read_usize();
            for _si in 0..onexit_len {
                state.onexit.push(self.read_executable_content_id());
            }
        }

        let transition_len = self.reader.read_usize();
        for _ti in 0..transition_len {
            state.transitions.push(self.read_transition_id());
        }

        if (flags & FSM_PROTOCOL_FLAG_INVOKE) != 0 {
            let invoke_len = self.reader.read_usize();
            for _ii in 0..invoke_len {
                let mut invoke = Invoke::new();
                self.read_invoke(&mut invoke);
                state.invoke.push(invoke);
            }
        }

        if (flags & FSM_PROTOCOL_FLAG_HISTORY) != 0 {
            let history_len = self.reader.read_usize();
            for _hi in 0..history_len {
                state.history.push(self.read_state_id());
            }
        }

        if (flags & FSM_PROTOCOL_FLAG_DATA) != 0 {
            self.read_data_store(&mut state.data);
        }

        state.parent = self.read_state_id();

        if (flags & FSM_PROTOCOL_FLAG_DONE_DATA) != 0 {
            let mut donedata = DoneData::new();
            self.read_done_data(&mut donedata);
            let _ = state.donedata.insert(donedata);
        } else {
            state.donedata = None;
        }

        #[cfg(feature = "Debug_Serializer")]
        debug!("<<State");
    }

    pub fn read_executable_content(&mut self) -> Box<dyn ExecutableContent> {
        let ec_type = self.reader.read_u8();

        match ec_type {
            executable_content::TYPE_IF => self.read_executable_content_if(),
            executable_content::TYPE_EXPRESSION => self.read_executable_content_expression(),
            executable_content::TYPE_SCRIPT => self.read_executable_content_script(),
            executable_content::TYPE_LOG => self.read_executable_content_log(),
            executable_content::TYPE_FOREACH => self.read_executable_content_for_each(),
            executable_content::TYPE_SEND => self.read_executable_content_send(),
            executable_content::TYPE_RAISE => self.read_executable_content_raise(),
            executable_content::TYPE_CANCEL => self.read_executable_content_cancel(),
            executable_content::TYPE_ASSIGN => self.read_executable_content_assign(),
            ut => {
                panic!("Unknown Executable Content: {}", ut)
            }
        }
    }

    pub fn read_executable_content_if(&mut self) -> Box<dyn ExecutableContent> {
        todo!()
    }

    pub fn read_executable_content_expression(&mut self) -> Box<dyn ExecutableContent> {
        todo!()
    }

    pub fn read_executable_content_script(&mut self) -> Box<dyn ExecutableContent> {
        todo!()
    }
    pub fn read_executable_content_log(&mut self) -> Box<dyn ExecutableContent> {
        let label = self.reader.read_string();
        let expression = self.reader.read_string();
        Box::new(Log::new(&Some(&label), &expression))
    }
    pub fn read_executable_content_for_each(&mut self) -> Box<dyn ExecutableContent> {
        todo!()
    }
    pub fn read_executable_content_send(&mut self) -> Box<dyn ExecutableContent> {
        todo!()
    }
    pub fn read_executable_content_raise(&mut self) -> Box<dyn ExecutableContent> {
        todo!()
    }
    pub fn read_executable_content_cancel(&mut self) -> Box<dyn ExecutableContent> {
        todo!()
    }
    pub fn read_executable_content_assign(&mut self) -> Box<dyn ExecutableContent> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    pub const FSM_SRC: &str = r###"
<?xml version="1.0" encoding="UTF-8"?>
<!-- A Simple FSM that wait for some event -->
<scxml xmlns="http://www.w3.org/2005/07/scxml" initial="s0" version="1.0" datamodel="ecmascript">
 <state id="s0">
  <transition event="go" target="s1"/>
 </state>
 <state id="s1">
  <transition event="go" target="s2"/>
 </state>
 <state id="s2">
  <transition event="go" target="end"/>
 </state>
 <final id="end">
  <onentry>
    <log expr="'Finished!!!'"/>
  </onentry>
 </final>
</scxml>"###;

    use super::*;
    use crate::scxml_reader;
    use crate::serializer::default_protocol_reader::DefaultProtocolReader;
    use crate::serializer::default_protocol_writer::DefaultProtocolWriter;
    use crate::serializer::fsm_writer::FsmWriter;

    struct TestEnvironment {
        fsm: Box<Fsm>,
        buffer: Vec<u8>,
    }

    fn setup() -> TestEnvironment {
        let fsm = scxml_reader::parse_from_xml(FSM_SRC.to_string()).unwrap();
        let mut writer: FsmWriter<Vec<u8>> =
            FsmWriter::new(Box::new(DefaultProtocolWriter::new(Vec::new())));
        writer.write(&fsm);
        writer.close();

        let buffer = writer.get_writer();

        TestEnvironment {
            fsm,
            buffer: buffer.clone(),
        }
    }

    #[test]
    fn shall_read_fsm<'a>() {
        let env = setup();
        let r = Box::new(DefaultProtocolReader::new(&env.buffer[..]));
        let mut fsm_reader = FsmReader::new(r);
        let result = fsm_reader.read();

        assert!(result.is_ok());

        assert!(env.fsm.compare_to(result.unwrap().as_ref()))
    }
}
