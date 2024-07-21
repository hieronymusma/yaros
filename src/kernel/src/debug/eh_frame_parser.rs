#![allow(dead_code)]

use alloc::{collections::BTreeMap, sync::Arc};
use common::{
    consumable_buffer::ConsumableBuffer,
    leb128::{SignedLEB128, UnsignedLEB128},
};

use crate::println;

pub struct EhFrameParser<'a> {
    data: &'a [u8],
}

impl<'a> EhFrameParser<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    pub fn iter(&self) -> EhFrameIterator<'a> {
        EhFrameIterator {
            data: ConsumableBuffer::new(self.data),
            parsed_cie: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
pub struct ParsedCIE<'a> {
    version: u8,
    augmentation_string: &'a str,
    eh_data: Option<usize>,
    address_size: u8,
    segment_size: u8,
    code_alignment_factor: u64,
    data_alignment_factor: i64,
    augmentation_data: Option<&'a [u8]>,
    return_address_register: u64,
    initial_instructions: &'a [u8],
}

#[derive(Debug)]
pub struct ParsedFDE;

#[derive(Debug)]
pub enum EhFrameEntry<'a> {
    CIE(Arc<ParsedCIE<'a>>),
    FDE(ParsedFDE),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Bitness {
    Bits64,
    Bits32,
}

pub struct EhFrameIterator<'a> {
    data: ConsumableBuffer<'a>,
    parsed_cie: BTreeMap<usize, Arc<ParsedCIE<'a>>>,
}

impl<'a> Iterator for EhFrameIterator<'a> {
    type Item = EhFrameEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let offset_of_cie = self.data.position();
        let (mut length, bitness) = self.parse_length_and_bitness()?;
        assert_eq!(
            bitness,
            Bitness::Bits32,
            "Currently only 32 Bit DWARF Format is implemented."
        );

        println!("Got {length} length");

        let cie_offset_or_none = self.data.consume_sized_type::<u32>()? as usize;
        println!("Got {cie_offset_or_none} offset");
        length -= 4;

        if cie_offset_or_none == 0 {
            self.parse_cie(length, offset_of_cie)
                .map(|e| EhFrameEntry::CIE(e))
        } else {
            self.parse_fde(cie_offset_or_none, length)
                .map(|e| EhFrameEntry::FDE(e))
        }
    }
}

impl<'a> EhFrameIterator<'a> {
    fn parse_cie(&mut self, length: usize, offset_of_cie: usize) -> Option<Arc<ParsedCIE<'a>>> {
        let begin_position = self.data.position();
        let version = self.data.consume_sized_type::<u8>()?;
        assert!(version == 1);
        let augmentation_string = self.data.consume_str()?;

        let eh_data = if augmentation_string.contains("eh") {
            Some(self.data.consume_sized_type::<usize>()?)
        } else {
            None
        };

        let address_size = core::mem::size_of::<usize>() as u8;
        let segment_size = 0;
        let code_alignment_factor = self.data.consume_unsized_type::<UnsignedLEB128>()?.get();
        let data_alignment_factor = self.data.consume_unsized_type::<SignedLEB128>()?.get();
        let return_address_register = self.data.consume_unsized_type::<UnsignedLEB128>()?.get();

        let augmentation_data = if augmentation_string.contains('z') {
            let length = self.data.consume_unsized_type::<UnsignedLEB128>()?.get();
            Some(self.data.consume_slice(length as usize)?)
        } else {
            None
        };

        // parse augmented data
        let current_position = self.data.position();
        let size = current_position - begin_position;
        let initial_instructions = self.data.consume_slice(length - size)?;

        let parsed_cie = ParsedCIE {
            version,
            augmentation_string,
            eh_data,
            address_size,
            segment_size,
            code_alignment_factor,
            data_alignment_factor,
            augmentation_data,
            return_address_register,
            initial_instructions,
        };

        let arced_cie = Arc::new(parsed_cie);

        println!("Insert at {offset_of_cie}");
        let already_exist = self.parsed_cie.insert(offset_of_cie, arced_cie.clone());

        assert!(
            already_exist.is_none(),
            "There should be only one CIE at each offset value."
        );

        Some(arced_cie)
    }

    fn parse_fde(&mut self, cie_offset: usize, length: usize) -> Option<ParsedFDE> {
        let fde_position = self.data.position();

        // We already read the CIE_POINTER field therefore, we need to remove 4 bytes
        let target_cie_location = fde_position - cie_offset - 4;
        println!("Target cie location {target_cie_location}");

        // let real_cie_offset = begin_position - offset_cie;
        let cie = self
            .parsed_cie
            .get(&target_cie_location)
            .expect("The corresponding entry must exist.")
            .clone();

        // Parse encoding of PC Begin
        let pc_begin = if cie.augmentation_string.contains('R') {
            0u32
        } else {
            self.data.consume_sized_type::<u32>()?
        }

        todo!();
    }

    fn parse_length_and_bitness(&mut self) -> Option<(usize, Bitness)> {
        let length = self.data.consume_sized_type::<u32>()?;

        if length != 0xffffffff {
            return Some((length as usize, Bitness::Bits32));
        }

        let length = self.data.consume_sized_type::<u64>()?;
        Some((length as usize, Bitness::Bits64))
    }
}

#[cfg(test)]
mod tests {
    use alloc::sync::Arc;

    use crate::{
        debug::eh_frame_parser::{EhFrameEntry, ParsedCIE},
        println,
    };
    use core::assert_matches::assert_matches;

    use super::EhFrameParser;

    const TEST_EH_FRAME: &[u8] = include_bytes!("../test/test_data/elf/.eh_frame");

    #[test_case]
    fn parser_works() {
        let parser = EhFrameParser::new(TEST_EH_FRAME);
        let mut entries = parser.iter();

        let cie = entries.next().expect("The first entry should exist");

        let expect = Arc::new(ParsedCIE {
            version: 1,
            augmentation_string: "zR",
            address_size: 8,
            segment_size: 0,
            code_alignment_factor: 1,
            data_alignment_factor: -8,
            return_address_register: 1,
            initial_instructions: &[0x0c, 0x02, 0x00],
            eh_data: None,
            augmentation_data: Some(&[0x1b]),
        });
        assert_matches!(cie, EhFrameEntry::CIE(expect));

        let fde = entries.next().expect("The second entry should exist");
    }
}
