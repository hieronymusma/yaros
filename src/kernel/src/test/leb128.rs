#[cfg(test)]
mod tests {
    use common::{
        consumable_buffer::ConsumableBuffer,
        leb128::{SignedLEB128, UnsignedLEB128},
    };

    #[test_case]
    fn unsigned_leb128() {
        const INPUT: &[u8] = &[0xe5, 0x8e, 0x26];

        let mut buffer = ConsumableBuffer::new(INPUT);
        let value = buffer
            .consume_unsized_type::<UnsignedLEB128>()
            .expect("Type must be consumable")
            .get()
            .expect("Value must be parsable.");
        assert_eq!(value, 624485);
    }

    #[test_case]
    fn signed_leb128() {
        const INPUT: &[u8] = &[0xc0, 0xbb, 0x78];

        let mut buffer = ConsumableBuffer::new(INPUT);
        let value = buffer
            .consume_unsized_type::<SignedLEB128>()
            .expect("Type must be consumable")
            .get()
            .expect("Value must be parsable.");
        assert_eq!(value, -123456);
    }

    #[test_case]
    fn unsigned_leb128_zero() {
        const INPUT: &[u8] = &[0x0];
        let mut buffer = ConsumableBuffer::new(INPUT);
        let value = buffer
            .consume_unsized_type::<UnsignedLEB128>()
            .expect("Type must be consumable")
            .get()
            .expect("Value must be parsable.");
        assert_eq!(value, 0);
    }

    #[test_case]
    fn signed_leb128_zero() {
        const INPUT: &[u8] = &[0x0];
        let mut buffer = ConsumableBuffer::new(INPUT);
        let value = buffer
            .consume_unsized_type::<SignedLEB128>()
            .expect("Type must be consumable")
            .get()
            .expect("Value must be parsable.");
        assert_eq!(value, 0);
    }
}
