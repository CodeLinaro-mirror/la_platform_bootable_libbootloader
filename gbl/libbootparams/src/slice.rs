// Copyright 2026, The Android Open Source Project
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

//! Helper for writing to a byte slice.

/// A helper for writing to a byte slice.
pub struct SliceWriter<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl<'a> SliceWriter<'a> {
    /// Creates a new `SliceWriter`.
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, len: 0 }
    }

    /// Returns the number of bytes written.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns the remaining part of the buffer.
    fn remainder(&mut self) -> &mut [u8] {
        &mut self.buf[self.len..]
    }
}

impl<'a> core::fmt::Write for SliceWriter<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let rem = self.remainder();
        rem.get_mut(..bytes.len()).ok_or(core::fmt::Error)?.copy_from_slice(bytes);
        self.len += bytes.len();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::fmt::Write;

    #[test]
    fn test_slice_writer() {
        let mut buf = [0u8; 10];
        let mut writer = SliceWriter::new(&mut buf);
        write!(writer, "abc").unwrap();
        assert_eq!(writer.len(), 3);
        write!(writer, "def").unwrap();
        assert_eq!(writer.len(), 6);
        assert_eq!(&buf[..6], b"abcdef");
    }

    #[test]
    fn test_slice_writer_overflow() {
        let mut buf = [0u8; 5];
        let mut writer = SliceWriter::new(&mut buf);
        assert!(write!(writer, "abcdef").is_err());
    }
}
