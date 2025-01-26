//! Default implementation of the read-protocol.\
//! The format is independent of the platform byte-order

use crate::serializer::default_protocol_definitions::*;
use crate::serializer::protocol_reader::ProtocolReader;
use byteorder::ReadBytesExt;
use std::collections::HashMap;

#[cfg(feature = "Debug_Serializer")]
use crate::common::debug;

use crate::common::error;
use crate::datamodel::{Data, SourceCode};
use std::io::Read;

pub struct DefaultProtocolReader<R>
where
    R: Read,
{
    reader: R,
    ok: bool,
    type_and_value: TypeAndValue,
    buffer: [u8; 4096],
}

#[derive(Default)]
pub struct TypeAndValue {
    type_id: u8,
    number: u64,
    string: String,
}

impl<R: Read> DefaultProtocolReader<R> {
    pub fn new(reader: R) -> DefaultProtocolReader<R> {
        DefaultProtocolReader {
            reader,
            ok: true,
            type_and_value: Default::default(),
            buffer: [0u8; 4096],
        }
    }

    fn verify_number_type(&mut self) -> bool {
        if self.ok {
            match self.type_and_value.type_id {
                FSM_PROTOCOL_TYPE_INT_4BIT
                | FSM_PROTOCOL_TYPE_INT_12BIT
                | FSM_PROTOCOL_TYPE_INT_20BIT
                | FSM_PROTOCOL_TYPE_INT_28BIT
                | FSM_PROTOCOL_TYPE_INT_36BIT
                | FSM_PROTOCOL_TYPE_INT_44BIT
                | FSM_PROTOCOL_TYPE_INT_52BIT
                | FSM_PROTOCOL_TYPE_INT_60BIT
                | FSM_PROTOCOL_TYPE_INT_68BIT => true,
                _ => {
                    self.error(format!("Expected numeric type, got {}", self.type_and_value.type_id).as_str());
                    false
                }
            }
        } else {
            false
        }
    }

    fn verify_string_type(&mut self) -> bool {
        if self.ok {
            match self.type_and_value.type_id {
                FSM_PROTOCOL_TYPE_STRING_LENGTH_4BIT | FSM_PROTOCOL_TYPE_STRING_LENGTH_12BIT => true,
                _ => {
                    self.error(format!("Expected string type, got #{}", self.type_and_value.type_id).as_str());
                    false
                }
            }
        } else {
            false
        }
    }

    fn error(&mut self, err: &str) {
        if self.ok {
            error!("{}", err);
            self.ok = false;
            self.type_and_value.type_id = 0;
            self.type_and_value.number = 0;
            self.type_and_value.string.clear();
        }
    }

    fn read_additional_number_bytes(&mut self, mut length: u8) {
        while length > 0 && self.ok {
            match self.reader.read_u8() {
                Ok(value) => {
                    self.type_and_value.number = (self.type_and_value.number << 8) | (value as u64);
                }
                Err(err) => {
                    self.error(format!("Error reading: {}", err).as_str());
                }
            }
            length -= 1;
        }
    }

    fn read_data_value_payload(&mut self, what: u8) -> Data {
        match what {
            0 => Data::Null(),
            1 => {
                let rv = self.read_string();
                match rv.parse::<i64>() {
                    Ok(val) => Data::Integer(val),
                    Err(err) => {
                        self.error(format!("Protocol error in Integer data value: {} -> {}", rv, err).as_str());
                        self.ok = false;
                        Data::Null()
                    }
                }
            }
            2 => {
                let rv = self.read_string();
                match rv.parse::<f64>() {
                    Ok(val) => Data::Double(val),
                    Err(err) => {
                        self.error(format!("Protocol error in Double data value: {} -> {}", rv, err).as_str());
                        self.ok = false;
                        Data::Null()
                    }
                }
            }
            3 => Data::String(self.read_string()),
            4 => Data::Boolean(self.read_boolean()),
            5 => {
                let len = self.read_usize();
                let mut val = Vec::with_capacity(len);
                for _i in 0..len {
                    val.push(self.read_data_arc());
                }
                Data::Array(val)
            }
            6 => {
                let len = self.read_usize();
                let mut val = HashMap::with_capacity(len);
                for _i in 0..len {
                    let k = self.read_string();
                    val.insert(k, self.read_data_arc());
                }
                Data::Map(val)
            }
            7 => {
                let k = self.read_string();
                Data::Error(k)
            }
            8 => {
                let k = self.read_string();
                let id = self.read_usize();
                Data::Source(SourceCode::new(k.as_str(), id))
            }
            9 => Data::None(),

            _ => {
                self.error(format!("Protocol error in data value: unknown variant {}", what).as_str());
                self.ok = false;
                Data::Null()
            }
        }
    }

    fn read_type_and_size(&mut self) {
        if self.ok {
            self.type_and_value.string.clear();
            match self.reader.read_u8() {
                Ok(val) => match val & 0xF0 {
                    0x10 => {
                        self.type_and_value.type_id = val;
                    }
                    FSM_PROTOCOL_TYPE_INT_4BIT => {
                        self.type_and_value.type_id = FSM_PROTOCOL_TYPE_INT_4BIT;
                        self.type_and_value.number = (val & 0x0F) as u64;
                    }
                    FSM_PROTOCOL_TYPE_INT_12BIT => {
                        self.type_and_value.type_id = FSM_PROTOCOL_TYPE_INT_12BIT;
                        self.type_and_value.number = (val & 0x0F) as u64;
                        self.read_additional_number_bytes(1);
                    }
                    FSM_PROTOCOL_TYPE_INT_20BIT => {
                        self.type_and_value.type_id = FSM_PROTOCOL_TYPE_INT_20BIT;
                        self.type_and_value.number = (val & 0x0F) as u64;
                        self.read_additional_number_bytes(2);
                    }
                    FSM_PROTOCOL_TYPE_INT_28BIT => {
                        self.type_and_value.type_id = FSM_PROTOCOL_TYPE_INT_28BIT;
                        self.type_and_value.number = (val & 0x0F) as u64;
                        self.read_additional_number_bytes(3);
                    }
                    FSM_PROTOCOL_TYPE_INT_36BIT => {
                        self.type_and_value.type_id = FSM_PROTOCOL_TYPE_INT_36BIT;
                        self.type_and_value.number = (val & 0x0F) as u64;
                        self.read_additional_number_bytes(4);
                    }
                    FSM_PROTOCOL_TYPE_INT_44BIT => {
                        self.type_and_value.type_id = FSM_PROTOCOL_TYPE_INT_44BIT;
                        self.type_and_value.number = (val & 0x0F) as u64;
                        self.read_additional_number_bytes(5);
                    }
                    FSM_PROTOCOL_TYPE_INT_52BIT => {
                        self.type_and_value.type_id = FSM_PROTOCOL_TYPE_INT_52BIT;
                        self.type_and_value.number = (val & 0x0F) as u64;
                        self.read_additional_number_bytes(6);
                    }
                    FSM_PROTOCOL_TYPE_INT_60BIT => {
                        self.type_and_value.type_id = FSM_PROTOCOL_TYPE_INT_60BIT;
                        self.type_and_value.number = (val & 0x0F) as u64;
                        self.read_additional_number_bytes(7);
                    }
                    FSM_PROTOCOL_TYPE_INT_68BIT => {
                        self.type_and_value.type_id = FSM_PROTOCOL_TYPE_INT_68BIT;
                        self.type_and_value.number = (val & 0x0F) as u64;
                        self.read_additional_number_bytes(8);
                    }
                    FSM_PROTOCOL_TYPE_STRING_LENGTH_4BIT => {
                        self.type_and_value.type_id = FSM_PROTOCOL_TYPE_STRING_LENGTH_4BIT;
                        self.type_and_value.number = 0;
                        let us = (val & 0x0F) as usize;
                        match self.reader.read_exact(&mut self.buffer[0..us]) {
                            Ok(_) => match std::str::from_utf8(&self.buffer[0..us]) {
                                Ok(val) => {
                                    self.type_and_value.string.insert_str(0, val);
                                }
                                Err(err_utf) => {
                                    self.error(format!("Error in utf8 sequence: {}", err_utf).as_str());
                                }
                            },
                            Err(err) => {
                                self.error(format!("Error reading: {}", err).as_str());
                                self.ok = false;
                            }
                        }
                    }
                    FSM_PROTOCOL_TYPE_STRING_LENGTH_12BIT => {
                        self.type_and_value.type_id = FSM_PROTOCOL_TYPE_STRING_LENGTH_12BIT;
                        self.type_and_value.number = 0;
                        let mut us = (val & 0x0F) as usize;

                        match self.reader.read_u8() {
                            Ok(value) => {
                                us = (us << 8) | (value as usize);
                                match self.reader.read_exact(&mut self.buffer[0..us]) {
                                    Ok(_) => match std::str::from_utf8(&self.buffer[0..us]) {
                                        Ok(val) => {
                                            self.type_and_value.string.insert_str(0, val);
                                        }
                                        Err(err_utf) => {
                                            self.error(format!("Error in utf8 sequence: {}", err_utf).as_str());
                                        }
                                    },
                                    Err(err) => {
                                        self.error(format!("Error reading: {}", err).as_str());
                                        self.ok = false;
                                    }
                                }
                            }
                            Err(err) => {
                                self.error(format!("Error reading: {}", err).as_str());
                            }
                        }
                    }
                    _ => {}
                },
                Err(e) => {
                    self.error(format!("Error reading: {}", e).as_str());
                }
            }
        }
    }
}

impl<R: Read> ProtocolReader<R> for DefaultProtocolReader<R> {
    fn verify_version(&mut self) {
        let vs = self.read_string();
        if !vs.eq(FSM_PROTOCOL_TYPE_PROTOCOL_VERSION) {
            self.error(format!("Wrong protocol version '{}'", vs).as_str());
        }
    }

    fn close(&mut self) {}

    fn read_boolean(&mut self) -> bool {
        if self.ok {
            match self.reader.read_u8() {
                Ok(type_id) => match type_id {
                    FSM_PROTOCOL_TYPE_BOOLEAN_TRUE => true,
                    FSM_PROTOCOL_TYPE_BOOLEAN_FALSE => false,
                    _ => {
                        self.error(format!("Expected bool, got {}", type_id).as_str());
                        false
                    }
                },
                Err(err) => {
                    self.error(format!("Error reading: {}", err).as_str());
                    false
                }
            }
        } else {
            false
        }
    }

    fn read_option_string(&mut self) -> Option<String> {
        if self.ok {
            self.read_type_and_size();
            return match self.type_and_value.type_id {
                FSM_PROTOCOL_TYPE_OPT_STRING_NONE => None,
                FSM_PROTOCOL_TYPE_STRING_LENGTH_12BIT | FSM_PROTOCOL_TYPE_STRING_LENGTH_4BIT => {
                    Some(self.type_and_value.string.clone())
                }
                _ => {
                    self.error(format!("Expected string, got {}", self.type_and_value.type_id).as_str());
                    None
                }
            };
        }
        None
    }

    fn read_data(&mut self) -> Data {
        let what = self.read_u8();
        self.read_data_value_payload(what)
    }

    fn read_string(&mut self) -> String {
        self.read_type_and_size();
        #[cfg(feature = "Debug_Serializer")]
        debug!("String {}", self.type_and_value.string);
        if self.verify_string_type() {
            self.type_and_value.string.clone()
        } else {
            "".to_string()
        }
    }

    fn read_usize(&mut self) -> usize {
        self.read_type_and_size();
        if self.verify_number_type() {
            self.type_and_value.number as usize
        } else {
            0
        }
    }

    /// Reads an unsigned values
    fn read_uint(&mut self) -> u64 {
        self.read_type_and_size();
        if self.verify_number_type() {
            self.type_and_value.number
        } else {
            0
        }
    }

    fn has_error(&self) -> bool {
        !self.ok
    }
}
