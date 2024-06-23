use crate::consumable_buffer::{ConsumableBuffer, FromU8BufferUnsized};

#[derive(Clone, Copy)]
pub struct UnsignedLEB128(Option<u64>);

impl UnsignedLEB128 {
    pub fn get(self) -> Option<u64> {
        self.0
    }

    fn parse(buffer: &[u8]) -> Option<u64> {
        let mut result = 0u64;
        let mut shift = 0u8;

        let mut buffer = ConsumableBuffer::new(buffer);

        loop {
            let next_byte = buffer.consume_sized_type::<u8>()? as u64;
            result |= (next_byte & 0b1111111) << shift;
            if next_byte & 0b10000000 == 0 {
                return Some(result);
            }
            shift += 7;
            if shift >= 64 {
                return None;
            }
        }
    }
}

impl FromU8BufferUnsized for UnsignedLEB128 {
    fn from_u8_buffer(buffer: &[u8]) -> Self {
        let parsed = UnsignedLEB128::parse(buffer);
        UnsignedLEB128(parsed)
    }
}

#[derive(Clone, Copy)]
pub struct SignedLEB128(Option<i64>);

impl SignedLEB128 {
    pub fn get(self) -> Option<i64> {
        self.0
    }

    fn parse(buffer: &[u8]) -> Option<i64> {
        let mut result = 0i64;
        let mut shift = 0u8;
        let size = 64;

        let mut buffer = ConsumableBuffer::new(buffer);
        let mut next_byte: i64;
        loop {
            next_byte = buffer.consume_sized_type::<u8>()? as i64;
            result |= (next_byte & 0b1111111) << shift;
            shift += 7;
            if shift >= size {
                return None;
            }
            if next_byte & 0b10000000 == 0 {
                break;
            }
        }

        if shift < size && next_byte & 0x40 != 0 {
            result |= !0 << shift;
        }
        Some(result)
    }
}

impl FromU8BufferUnsized for SignedLEB128 {
    fn from_u8_buffer(buffer: &[u8]) -> Self {
        let parsed = SignedLEB128::parse(buffer);
        SignedLEB128(parsed)
    }
}
