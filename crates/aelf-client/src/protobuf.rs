//! Internal protobuf helpers shared across client-facing crates.

#![forbid(unsafe_code)]

use prost::Message;

/// Write-only protobuf adapter for pre-encoded message bytes.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawBytesMessage(Vec<u8>);

impl RawBytesMessage {
    /// Creates a new write-only message wrapper from already encoded protobuf bytes.
    #[doc(hidden)]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl Message for RawBytesMessage {
    fn encode_raw(&self, buf: &mut impl prost::bytes::BufMut) {
        buf.put_slice(&self.0);
    }

    fn merge_field(
        &mut self,
        _tag: u32,
        _wire_type: prost::encoding::WireType,
        _buf: &mut impl prost::bytes::Buf,
        _ctx: prost::encoding::DecodeContext,
    ) -> Result<(), prost::DecodeError> {
        unreachable!("RawBytesMessage is write-only")
    }

    fn encoded_len(&self) -> usize {
        self.0.len()
    }

    fn clear(&mut self) {
        self.0.clear();
    }
}
