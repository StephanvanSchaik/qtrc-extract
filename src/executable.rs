use anyhow::Result;
use goblin::Object;
use goblin::elf::program_header::PT_LOAD;
use rangemap::RangeMap;
use std::collections::BTreeMap;
use std::ops::Range;
use std::path::PathBuf;

pub struct ExecutableMapping {
    /// The preferred image base.
    image_base: usize,
    /// Maps virtual addresses to file offsets.
    rva_mapping: RangeMap<usize, usize>,
    /// Maps file offsets to virtual addresses.
    file_mapping: RangeMap<usize, usize>,
}

impl ExecutableMapping {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut image_base = 0;
        let mut rva_mapping = RangeMap::new();
        let mut file_mapping = RangeMap::new();

        match Object::parse(&bytes)? {
            Object::Elf(elf) => {
                for segment in elf.program_headers {
                    if segment.p_type != PT_LOAD || segment.p_filesz == 0 || segment.p_memsz == 0 {
                        continue;
                    }

                    // Calculate the file range of this section.
                    let start = segment.p_offset as usize;
                    let end = start + segment.p_filesz as usize;
                    let file_range = start..end;

                    // Calculate the virtual address range of this section.
                    let start = segment.p_vaddr as usize;
                    let end = start + segment.p_memsz as usize;
                    let rva_range = start..end;

                    // Track the mappings.
                    file_mapping.insert(file_range.clone(), rva_range.start);
                    rva_mapping.insert(rva_range, file_range.start);
                }
            }
            Object::PE(pe) => {
                image_base = pe.image_base as usize;

                for section in pe.sections {
                    // Calculate the file range of this section.
                    let start = section.pointer_to_raw_data as usize;
                    let end = start + section.size_of_raw_data as usize;
                    let file_range = start..end;

                    // Calculate the virtual address range of this section.
                    let start = section.virtual_address as usize;
                    let end = start + section.virtual_size as usize;
                    let rva_range = start..end;

                    // Track the mappings.
                    file_mapping.insert(file_range.clone(), rva_range.start);
                    rva_mapping.insert(rva_range, file_range.start);
                }
            }
            _ => (),
        }

        Ok(Self {
            image_base,
            rva_mapping,
            file_mapping,
        })
    }

    /// Calculates the file offset from the virtual address.
    pub fn rva_to_file_offset(&self, rva: usize) -> Option<usize> {
        let rva = rva - self.image_base;

        let (rva_range, file_base) = match self.rva_mapping.get_key_value(&rva) {
            Some(segment) => segment,
            _ => return None,
        };

        Some(rva + file_base - rva_range.start)
    }

    /// Calculates the virtual address from the file offset.
    pub fn file_offset_to_rva(&self, file_offset: usize) -> Option<usize> {
        let (file_range, rva_base) = match self.file_mapping.get_key_value(&file_offset) {
            Some(segment) => segment,
            _ => return None,
        };

        Some(file_offset + rva_base + self.image_base - file_range.start)
    }
}
