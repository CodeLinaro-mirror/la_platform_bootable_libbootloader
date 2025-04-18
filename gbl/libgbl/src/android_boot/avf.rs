// Copyright 2025, The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Android virtualization framework support for GBL

use super::load::BootImageV3Info;
use crate::constants::PAGE_SIZE;
use crate::image_buffer::ImageBuffer;
use crate::{GblOps, Result};
use core::ffi::CStr;
use liberror::Error;

pub const DEFAULT_PVMFW_PART_NAME_CSTR: &CStr = c"pvmfw";
pub const DEFAULT_PVMFW_PART_NAME: &str = "pvmfw";

/// Places pvmfw firmware binary into reserved memory region
pub fn pvmfw_place_in_memory<'a, 'b>(
    ops: &mut impl GblOps<'a, 'b>,
    pvmfw_partition_buf: &[u8],
) -> Result<ImageBuffer<'b>> {
    const PVMFW_RESVMEM_NAME: &str = "pvmfw_data";

    // Parse the partition header an extract the pvmfw binary
    let info = BootImageV3Info::new(pvmfw_partition_buf)?;
    let pvmfw_bin =
        pvmfw_partition_buf.get(info.kernel_range.clone()).ok_or(Error::BadBufferSize)?;
    let pvmfw_bin_size = pvmfw_bin.len();
    assert!(pvmfw_bin_size % PAGE_SIZE == 0, "expected page aligned buffer");

    let mut target_buf =
        ops.get_image_buffer(PVMFW_RESVMEM_NAME, pvmfw_bin_size.try_into().unwrap())?;

    // SAFETY: the used buffer will be fully initialized by writing pvmfw binary and padding
    unsafe { target_buf.advance_used(pvmfw_bin_size) }?;
    if target_buf.used().as_ptr().align_offset(PAGE_SIZE) != 0 {
        return Err(Error::InvalidAlignment.into());
    }
    target_buf.used_mut().copy_from_slice(pvmfw_bin);

    Ok(target_buf)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        ops::test::{FakeGblOps, FakeGblOpsStorage},
        tests::AlignedBuffer,
    };
    use gbl_storage::as_uninit_mut;
    use std::collections::{HashMap, LinkedList};

    fn dummy_pvmfw_partition(fill_value: u8) -> Vec<u8> {
        const HEADER_SIZE: usize = 4096;
        const PARTITION_SIZE: usize = 8192;

        let mut partition = vec![
            0x41, 0x4e, 0x44, 0x52, // magic
            0x4f, 0x49, 0x44, 0x21, // magic
            0x00, 0x0c, 0x00, 0x00, // kernel size (binary size)
            0x00, 0x00, 0x00, 0x00, // ramdisk size
            0x8b, 0x01, 0x00, 0x1e, // os version
            0x2c, 0x06, 0x00, 0x00, // header size
            0x00, 0x00, 0x00, 0x00, // reserved
            0x00, 0x00, 0x00, 0x00, // reserved
            0x00, 0x00, 0x00, 0x00, // reserved
            0x00, 0x00, 0x00, 0x00, // reserved
            0x03, 0x00, 0x00, 0x00, // version
        ];
        partition.resize(PARTITION_SIZE, 0);
        partition[HEADER_SIZE..HEADER_SIZE + 0xc00].fill(fill_value);
        partition
    }

    fn add_image_buffer<'a, 'b: 'a>(ops: &mut FakeGblOps<'_, 'a>, buf: &'b mut AlignedBuffer) {
        let buf_image = ImageBuffer::new(as_uninit_mut(buf.as_mut()));
        let mut list = LinkedList::<ImageBuffer>::new();
        list.push_back(buf_image);
        ops.image_buffers = HashMap::new();
        ops.image_buffers.insert("pvmfw_data".into(), list);
    }

    #[test]
    fn test_pvmfw_place_in_memory() {
        let mut pvmfw_buf_aligned = AlignedBuffer::new(0x100000, 0x1000);
        let storage = FakeGblOpsStorage::default();
        let mut ops = FakeGblOps::new(&storage);
        add_image_buffer(&mut ops, &mut pvmfw_buf_aligned);

        const FILL_VALUE: u8 = 0xAB;
        let mut pvmfw_partition = dummy_pvmfw_partition(FILL_VALUE);
        let reg = pvmfw_place_in_memory(&mut ops, &mut pvmfw_partition).unwrap();
        let used_bytes = reg.used();

        assert_eq!(used_bytes.len(), 4096);
        assert!(&used_bytes[..0xc00].iter().all(|&b| b == FILL_VALUE));
        assert!(&used_bytes[0xc00..].iter().all(|&b| b == 0u8));
    }

    #[test]
    fn test_pvmfw_place_in_memory_bad_header() {
        let mut pvmfw_buf_aligned = AlignedBuffer::new(0x100000, 0x1000);
        let storage = FakeGblOpsStorage::default();
        let mut ops = FakeGblOps::new(&storage);
        add_image_buffer(&mut ops, &mut pvmfw_buf_aligned);

        let mut pvmfw_partition = [0u8; 0x1000];
        assert!(pvmfw_place_in_memory(&mut ops, &mut pvmfw_partition).is_err());
    }
}
