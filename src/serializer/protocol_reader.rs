//! Protocol to read a persistent binary version of a Fsm.

use crate::datamodel::{create_data_arc, Data, DataArc};
use std::io::Read;

/// Trait for reading binary data in some platform independent way.\
/// The resulting data should be sharable with different systems (different OS, Byte-Order... whatever).
pub trait ProtocolReader<R: Read> {
    /// Reads and verify the protocol version
    /// Goes to error state if version doesn't match.
    fn verify_version(&mut self);

    /// Close the underlying stream
    fn close(&mut self);

    /// Reads a boolean
    fn read_boolean(&mut self) -> bool;

    /// Reads an optional string
    fn read_option_string(&mut self) -> Option<String>;

    /// Reads a Data (enum) value
    fn read_data_arc(&mut self) -> DataArc {
        create_data_arc(self.read_data())
    }

    /// Reads a Data (enum) value
    fn read_data(&mut self) -> Data;

    /// Reads a string
    fn read_string(&mut self) -> String;

    /// Reads an usize values.
    fn read_usize(&mut self) -> usize;

    /// Reads an unsigned values
    fn read_uint(&mut self) -> u64;

    fn read_u8(&mut self) -> u8 {
        let u = self.read_uint();
        u as u8
    }

    fn read_u16(&mut self) -> u16 {
        let u = self.read_uint();
        u as u16
    }

    fn read_u32(&mut self) -> u32 {
        let u = self.read_uint();
        u as u32
    }

    fn has_error(&self) -> bool;
}
