//! Default implementation of the write-protocol.\
//! The format is independent of the platform byte-order

use crate::serializer::default_protocol_definitions::*;
use crate::serializer::protocol_writer::ProtocolWriter;
use byteorder::WriteBytesExt;
use log::error;
use std::io::{BufWriter, Write};

pub struct DefaultProtocolWriter<W: Write> {
    writer: BufWriter<W>,
    in_error: bool,
}

impl<W: Write> DefaultProtocolWriter<W> {
    pub fn new(writer: BufWriter<W>) -> DefaultProtocolWriter<W> {
        DefaultProtocolWriter {
            writer,
            in_error: false,
        }
    }

    fn eval_result(&mut self, result: std::io::Result<()>) {
        match result {
            Ok(_v) => {}
            Err(err) => {
                error!("Error writing: {}", err);
                self.in_error = true;
            }
        }
    }

    fn write_type_and_value(&mut self, type_id: u8, value: u64, mut size: u8) {
        if !self.in_error {
            size = size.saturating_sub(4);
            let mut r = self
                .writer
                .write_u8(type_id | (((value >> size) as u8) & 0x0F));
            while size > 0 && r.is_ok() {
                r = self.writer.write_u8((value >> size) as u8);
                size = size.saturating_sub(8);
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
        if !self.in_error {
            let r = self.writer.flush();
            self.eval_result(r);
        }
    }

    fn write_boolean(&mut self, value: bool) {
        if !self.in_error {
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
        } else if !self.in_error {
            let r = self.writer.write_u8(FSM_PROTOCOL_TYPE_OPT_STRING_NONE);
            self.eval_result(r);
        }
    }

    fn write_str(&mut self, value: &str) {
        if !self.in_error {
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
                    self.eval_result(Result::Err(error));
                }
            }
        }
    }

    fn write_usize(&mut self, value: usize) {
        self.write_uint(value as u64)
    }

    fn write_uint(&mut self, value: u64) {
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
        self.in_error
    }
}
