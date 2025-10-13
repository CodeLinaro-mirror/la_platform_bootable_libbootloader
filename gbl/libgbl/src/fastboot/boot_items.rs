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

//! Boot item container. This module defines a custom container format used within GBL
//! to pass various boot-related items (like kernel command line arguments and bootconfig)
//! between different stages of the boot process. It is not dependent on any external
//! formats and is specific to GBL's internal workings.

use core::mem::{size_of, take};
use liberror::Error;
use safemath::SafeNum;
use zerocopy::{ByteSlice, FromBytes, Immutable, IntoBytes, KnownLayout, Ref, SplitByteSlice};

const BOOT_ITEM_CONTAINER_MAGIC: u64 = 0xc6157cb151cac1b4;

/// Represents the boot item container header.
///
/// Container format:
///
/// +---------------------------+
/// | BootItemContainerHeader   |
/// +---------------------------+
/// | First Item header         |
/// +---------------------------+
/// | First Item payload        |
/// +---------------------------+
/// ~ ...                       ~
/// +---------------------------+
#[repr(packed)]
#[derive(Debug, Copy, Clone, FromBytes, IntoBytes, Immutable, KnownLayout)]
struct BootItemContainerHeader {
    magic: u64,
    // Total size of the used part, excluding header.
    total_size: usize,
}

#[repr(usize)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum BootItem {
    Cmdline = 0,
    Bootconfig = 1,
}

impl TryFrom<usize> for BootItem {
    type Error = Error;

    fn try_from(val: usize) -> Result<Self, Self::Error> {
        Ok(match val {
            _ if val == BootItem::Cmdline as _ => BootItem::Cmdline,
            _ if val == BootItem::Bootconfig as _ => BootItem::Bootconfig,
            _ => return Err(Error::InvalidInput),
        })
    }
}

/// Header structure for boot item.
#[repr(packed)]
#[derive(Copy, Clone, FromBytes, IntoBytes, Immutable, KnownLayout)]
struct BootItemHeader {
    item_type: usize,
    payload_size: usize,
}

#[derive(Debug)]
pub(crate) struct BootItemContainer<'a>(&'a mut [u8]);

/// Helper for parsing header and payload.
fn parse_container<B: ByteSlice + SplitByteSlice>(
    buffer: B,
) -> Result<(Ref<B, BootItemContainerHeader>, B), Error> {
    Ref::<_, BootItemContainerHeader>::from_prefix(buffer).map_err(|_| Error::InvalidInput)
}

// TODO(b/450268123): Removed when integrated into android_load_verify_fixup.
#[allow(dead_code)]
impl<'a> BootItemContainer<'a> {
    /// Creates a new instance.
    pub(crate) fn new(buffer: &'a mut [u8]) -> Result<Self, Error> {
        buffer.get_mut(0).map(|v| *v = 0);
        Self::from(buffer, true)
    }

    /// Gets the raw buffer.
    pub(crate) fn raw_buffer(&mut self) -> &mut [u8] {
        self.0 as _
    }

    /// Creates an instance that borrows internal fields.
    pub(crate) fn as_borrowed(&mut self) -> BootItemContainer {
        BootItemContainer(self.0)
    }

    /// Initialized from a buffer with an existing container. Creates if none exists and `create`
    /// is true.
    pub(crate) fn from(buffer: &'a mut [u8], create: bool) -> Result<Self, Error> {
        let (mut header, payload) = parse_container(&mut buffer[..])?;
        match header.magic == BOOT_ITEM_CONTAINER_MAGIC && header.total_size <= payload.len() {
            true => Ok(BootItemContainer(buffer)),
            _ if create => {
                header.magic = BOOT_ITEM_CONTAINER_MAGIC;
                header.total_size = 0;
                Ok(BootItemContainer(buffer))
            }
            _ => Err(Error::InvalidInput),
        }
    }

    /// Initializes from a buffer with an existing container and splits out the unused buffer.
    pub(crate) fn from_prefix(buffer: &'a mut [u8], create: bool) -> (Option<Self>, &'a mut [u8]) {
        match BootItemContainer::from(buffer, create).is_ok() {
            true => {
                let mut res = Self::from(buffer, create).unwrap();
                let unused = res.split_unused();
                (Some(res), unused)
            }
            _ => return (None, buffer),
        }
    }

    /// Appends using the given construction closure.
    pub(crate) fn append_with(
        &mut self,
        f: impl FnOnce(&mut [u8]) -> Result<(BootItem, usize), Error>,
    ) -> Result<(), Error> {
        let (mut item_header, payload) =
            Ref::<_, BootItemHeader>::from_prefix(self.get_unused())
                .map_err(|_| Error::BufferTooSmall(Some(size_of::<BootItemHeader>())))?;
        let (item, sz) = f(payload)?;
        *item_header = BootItemHeader { item_type: item as _, payload_size: sz };
        let (mut header, _) = parse_container(&mut self.0[..]).unwrap();
        header.total_size =
            usize::try_from(SafeNum::from(sz) + size_of::<BootItemHeader>() + header.total_size)?;
        Ok(())
    }

    /// Appends a new item
    pub(crate) fn append_item(&mut self, item_type: BootItem, data: &[u8]) -> Result<(), Error> {
        self.append_with(|buf| {
            buf.get_mut(..data.len())
                .ok_or(Error::BufferTooSmall(Some(data.len())))?
                .clone_from_slice(data);
            Ok((item_type, data.len()))
        })
    }

    /// Returns an iterator to all items.
    pub(crate) fn iter(&self) -> BootItemContainerIter {
        let (header, payload) = parse_container(&self.0[..]).unwrap();
        BootItemContainerIter(&payload[..header.total_size])
    }

    /// Returns an iterator to specific items.
    pub(crate) fn item_iter(&self, item_type: BootItem) -> impl Iterator<Item = &[u8]> + Clone {
        self.iter().filter(move |(v, _)| *v == item_type).map(|(_, v)| v)
    }

    /// Returns an iterator to specific items and decode payload as utf8. Invalid utf8 string will
    /// be ignored.
    pub(crate) fn utf8_items(&self, item_type: BootItem) -> impl Iterator<Item = &str> + Clone {
        self.item_iter(item_type)
            .map(|v| core::str::from_utf8(v).unwrap_or(""))
            .filter(|v| *v != "")
    }

    /// Gets the unused buffer.
    pub(crate) fn get_unused(&mut self) -> &mut [u8] {
        let (header, payload) = parse_container(&mut self.0[..]).unwrap();
        &mut payload[header.total_size..]
    }

    /// Splits out the unused part of the buffer.
    pub(crate) fn split_unused(&mut self) -> &'a mut [u8] {
        let total_size = parse_container(&self.0[..]).unwrap().0.total_size;
        let (buffer, unused) =
            take(&mut self.0).split_at_mut(total_size + size_of::<BootItemContainerHeader>());
        self.0 = buffer;
        unused
    }
}

/// Iterator to items in boot container.
#[derive(Copy, Clone)]
pub(crate) struct BootItemContainerIter<'a>(&'a [u8]);

impl<'a> Iterator for BootItemContainerIter<'a> {
    type Item = (BootItem, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        let (header, payload) = Ref::<_, BootItemHeader>::from_prefix(self.0).ok()?;
        let (payload, next) = payload.split_at(header.payload_size);
        self.0 = next;
        Some((header.item_type.try_into().unwrap(), payload))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_append() {
        let mut buffer = vec![0u8; 4096];
        let mut container = BootItemContainer::new(&mut buffer[..]).unwrap();
        container.append_item(BootItem::Cmdline, "arg_0=val_0".as_bytes()).unwrap();
        container.append_item(BootItem::Cmdline, "arg_1=val_1".as_bytes()).unwrap();
        container.append_item(BootItem::Bootconfig, "arg_2=val_2".as_bytes()).unwrap();
        assert_eq!(
            container.iter().collect::<Vec<_>>(),
            vec![
                (BootItem::Cmdline, b"arg_0=val_0" as _),
                (BootItem::Cmdline, b"arg_1=val_1" as _),
                (BootItem::Bootconfig, b"arg_2=val_2" as _),
            ]
        );

        assert_eq!(
            container.utf8_items(BootItem::Cmdline).collect::<Vec<_>>(),
            ["arg_0=val_0", "arg_1=val_1"]
        );
        assert_eq!(container.utf8_items(BootItem::Bootconfig).collect::<Vec<_>>(), ["arg_2=val_2"]);
    }

    #[test]
    fn test_from() {
        let mut buffer = vec![0u8; 4096];
        let mut container = BootItemContainer::new(&mut buffer[..]).unwrap();
        container.append_item(BootItem::Cmdline, "arg_0=val_0".as_bytes()).unwrap();
        container.append_item(BootItem::Cmdline, "arg_1=val_1".as_bytes()).unwrap();
        container.append_item(BootItem::Bootconfig, "arg_2=val_2".as_bytes()).unwrap();
        assert_eq!(
            BootItemContainer::from(&mut buffer[..], false).unwrap().iter().collect::<Vec<_>>(),
            vec![
                (BootItem::Cmdline, b"arg_0=val_0" as _),
                (BootItem::Cmdline, b"arg_1=val_1" as _),
                (BootItem::Bootconfig, b"arg_2=val_2" as _),
            ]
        );
    }

    #[test]
    fn test_from_invalid() {
        let mut buffer = vec![0u8; 4096];
        assert!(BootItemContainer::from(&mut buffer[..], false).is_err());

        let (mut header, payload) =
            Ref::<_, BootItemContainerHeader>::from_prefix(&mut buffer[..]).unwrap();
        header.magic = BOOT_ITEM_CONTAINER_MAGIC;
        header.total_size = payload.len() + 1;
        assert!(BootItemContainer::from(&mut buffer[..], false).is_err());
    }

    #[test]
    fn test_buffer_too_small() {
        assert!(BootItemContainer::new(&mut vec![0u8; size_of::<BootItemContainerHeader>() - 1])
            .is_err());

        let mut buffer =
            vec![0u8; size_of::<BootItemContainerHeader>() + size_of::<BootItemHeader>() - 1];
        let mut container = BootItemContainer::new(&mut buffer[..]).unwrap();
        assert!(container.append_item(BootItem::Cmdline, b"").is_err());

        let mut buffer =
            vec![0u8; size_of::<BootItemContainerHeader>() + size_of::<BootItemHeader>()];
        let mut container = BootItemContainer::new(&mut buffer[..]).unwrap();
        assert!(container.append_item(BootItem::Cmdline, b"a").is_err());

        let mut buffer =
            vec![0u8; size_of::<BootItemContainerHeader>() + size_of::<BootItemHeader>() + 1];
        let mut container = BootItemContainer::new(&mut buffer[..]).unwrap();
        container.append_item(BootItem::Cmdline, b"a").unwrap();
        container.split_unused();
        assert!(container.append_item(BootItem::Cmdline, b"a").is_err());
    }
}
