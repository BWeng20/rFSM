//! Default implementation of the write-protocol.\
//! The format is independent of the platform byte-order

use std::io::Write;

#[cfg(feature = "Debug_Serializer")]
use crate::common::debug;
use crate::common::error;
use byteorder::WriteBytesExt;

use crate::datamodel::Data;
use crate::serializer::default_protocol_definitions::*;
use crate::serializer::protocol_writer::ProtocolWriter;

pub struct DefaultProtocolWriter<W> {
    pub writer: W,
    ok: bool,
}

impl<W: Write> DefaultProtocolWriter<W> {
    pub fn new(writer: W) -> DefaultProtocolWriter<W> {
        DefaultProtocolWriter { writer, ok: true }
    }

    fn eval_result(&mut self, result: std::io::Result<()>) {
        match result {
            Ok(_v) => {}
            Err(err) => {
                error!("Error writing: {}", err);
                self.ok = false;
            }
        }
    }

    fn write_type_and_value(&mut self, type_id: u8, value: u64, mut size: u8) {
        if self.ok {
            size = size.saturating_sub(4);
            let mut r = self
                .writer
                .write_u8(type_id | (((value >> size) as u8) & 0x0F));
            while size > 0 && r.is_ok() {
                size = size.saturating_sub(8);
                r = self.writer.write_u8((value >> size) as u8);
            }
            self.eval_result(r);
        }
    }
}

impl<W: Write> ProtocolWriter<W> for DefaultProtocolWriter<W> {
    fn write_version(&mut self) {
        self.write_str(FSM_PROTOCOL_TYPE_PROTOCOL_VERSION);
    }

    fn close(&mut self) {
        if self.ok {
            let r = self.writer.flush();
            self.eval_result(r);
        }
    }

    fn write_boolean(&mut self, value: bool) {
        if self.ok {
            #[cfg(feature = "Debug_Serializer")]
            debug!("BOOL {}", value);
            let r = self.writer.write_u8(if value {
                FSM_PROTOCOL_TYPE_BOOLEAN_TRUE
            } else {
                FSM_PROTOCOL_TYPE_BOOLEAN_FALSE
            });
            self.eval_result(r);
        }
    }

    fn write_option_string(&mut self, value: &Option<String>) {
        if value.is_some() {
            self.write_str(value.as_ref().unwrap().as_str());
        } else if self.ok {
            let r = self.writer.write_u8(FSM_PROTOCOL_TYPE_OPT_STRING_NONE);
            self.eval_result(r);
        }
    }

    fn write_data(&mut self, value: &Data) {
        match value {
            Data::Integer(val) => {
                self.write_u8(1);
                self.write_str(val.to_string().as_str());
            }
            Data::Double(val) => {
                self.write_u8(2);
                self.write_str(val.to_string().as_str());
            }
            Data::String(val) => {
                self.write_u8(3);
                self.write_str(val.as_str());
            }
            Data::Boolean(val) => {
                self.write_u8(4);
                self.write_boolean(*val);
            }
            Data::Array(val) => {
                self.write_u8(5);
                self.write_usize(val.len());
                for v in val {
                    self.write_data_arc(v);
                }
            }
            Data::Map(val) => {
                self.write_u8(6);
                self.write_usize(val.len());
                for (key, v) in val {
                    self.write_str(key.as_str());
                    self.write_data_arc(v);
                }
            }
            Data::Error(s) => {
                self.write_u8(7);
                self.write_str(s.as_str());
            }
            Data::Source(s) => {
                self.write_u8(8);
                self.write_str(s.source.as_str());
                self.write_usize(s.source_id);
            }
            Data::None() => {
                self.write_u8(9);
            }
            Data::Null() => {
                self.write_u8(0);
            }
        }
    }

    fn write_str(&mut self, value: &str) {
        if self.ok {
            #[cfg(feature = "Debug_Serializer")]
            debug!("String {}", value);
            let mut len = value.len();
            if len < (1usize << 4) {
                self.write_type_and_value(FSM_PROTOCOL_TYPE_STRING_LENGTH_4BIT, len as u64, 4);
            } else {
                self.write_type_and_value(FSM_PROTOCOL_TYPE_STRING_LENGTH_12BIT, len as u64, 12);
                len &= 0x0FFFusize;
            }
            let r = self.writer.write(value[0..len].as_bytes());
            match r {
                Ok(_) => {}
                Err(error) => {
                    self.eval_result(Err(error));
                }
            }
        }
    }

    fn write_usize(&mut self, value: usize) {
        self.write_uint(value as u64)
    }

    fn write_uint(&mut self, value: u64) {
        #[cfg(feature = "Debug_Serializer")]
        debug!("uint {}", value);
        if value < (1u64 << 4) {
            self.write_type_and_value(FSM_PROTOCOL_TYPE_INT_4BIT, value, 4);
        } else if value < (1u64 << 12) {
            self.write_type_and_value(FSM_PROTOCOL_TYPE_INT_12BIT, value, 12);
        } else if value < (1u64 << 20) {
            self.write_type_and_value(FSM_PROTOCOL_TYPE_INT_20BIT, value, 20);
        } else if value < (1u64 << 28) {
            self.write_type_and_value(FSM_PROTOCOL_TYPE_INT_28BIT, value, 28);
        } else if value < (1u64 << 36) {
            self.write_type_and_value(FSM_PROTOCOL_TYPE_INT_36BIT, value, 36);
        } else if value < (1u64 << 44) {
            self.write_type_and_value(FSM_PROTOCOL_TYPE_INT_44BIT, value, 44);
        } else if value < (1u64 << 52) {
            self.write_type_and_value(FSM_PROTOCOL_TYPE_INT_52BIT, value, 52);
        } else if value < (1u64 << 60) {
            self.write_type_and_value(FSM_PROTOCOL_TYPE_INT_60BIT, value, 60);
        } else {
            self.write_type_and_value(FSM_PROTOCOL_TYPE_INT_68BIT, value, 64);
        }
    }

    fn has_error(&self) -> bool {
        !self.ok
    }

    fn get_writer(&self) -> &W {
        &self.writer
    }
}
