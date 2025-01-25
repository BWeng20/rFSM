//! Low level protocol used to write the binary ruFsm format.

use crate::datamodel::{Data, DataArc};
use std::io::Write;

/// Trait for writing binary data in some platform independent way.\
/// The resulting data should be sharable with different systems (different OS, Byte-Order... whatever).
pub trait ProtocolWriter<W: Write> {
    /// Writes the protocol version
    fn write_version(&mut self);

    /// Flush and close the underlying stream
    fn close(&mut self);

    /// Writes a boolean
    fn write_boolean(&mut self, value: bool);

    /// Writes an optional string
    fn write_option_string(&mut self, value: &Option<String>);

    /// Writes a Data Value via an Arc
    fn write_data_arc(&mut self, value: &DataArc) {
        match value.lock() {
            Ok(guard) => {
                self.write_data(&guard);
            }
            Err(_) => {
                self.write_data(&Data::Null());
            }
        }
    }

    /// Writes a Data Value
    fn write_data(&mut self, value: &Data);

    /// Writes a str
    fn write_str(&mut self, value: &str);

    /// Writes an usize value. Implementations can assume that the value are in u32 range.
    fn write_usize(&mut self, value: usize);

    /// Writes an unsigned value
    fn write_uint(&mut self, value: u64);

    /// Writes an unsigned byte
    fn write_u8(&mut self, value: u8) {
        self.write_uint(value as u64)
    }

    fn has_error(&self) -> bool;

    fn get_writer(&self) -> &W;
}
