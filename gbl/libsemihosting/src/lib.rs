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

//! Contains API for semihosting
//!
//! For aarch64, see
//! https://github.com/ARM-software/abi-aa/blob/2982a9f3b512a5bfdc9e3fea5d3b298f9165c36b/semihosting/semihosting.rst
//!
//! For riscv64, see
//! https://docs.riscv.org/reference/platform-software/semihosting/_attachments/riscv-semihosting.pdf

#![cfg_attr(not(test), no_std)]

use core::{ffi::CStr, num::NonZeroUsize};
use liberror::Error;

/// Semihosting operation codes.
#[derive(Copy, Clone, Debug)]
#[repr(usize)]
enum OpCode {
    Open,
    Close,
    WriteC,
    Write,
    Read,
    FLen,
    System,
    ExitExtended,
}

impl From<OpCode> for usize {
    fn from(op_code: OpCode) -> Self {
        match op_code {
            OpCode::Open => 0x1,
            OpCode::Close => 0x2,
            OpCode::WriteC => 0x4,
            OpCode::Write => 0x5,
            OpCode::Read => 0x6,
            OpCode::FLen => 0x0C,
            OpCode::System => 0x12,
            OpCode::ExitExtended => 0x20,
        }
    }
}

/// Semihosting exit reasons.
#[derive(Copy, Clone, Debug)]
#[repr(usize)]
enum ExitReason {
    ApplicationExit = 0x20026,
}

/// Semihosting call for aarch64
#[cfg(target_arch = "aarch64")]
fn semihosting_call(op_code: OpCode, param: usize) -> Option<usize> {
    let mut ret = usize::from(op_code);
    // SAFETY: Semihosting API call. The operation does not modify program state.
    unsafe {
        core::arch::asm!(
            "hlt 0xF000",
            inout("x0") ret,
            in("x1") param,
            clobber_abi("C"),
        );
    };
    Some(ret)
}

#[cfg(target_arch = "riscv64")]
fn semihosting_call(_: OpCode, _: usize) -> Option<usize> {
    unimplemented!()
}

#[cfg(target_arch = "x86_64")]
fn semihosting_call(_: OpCode, _: usize) -> Option<usize> {
    // X86 has no such concept
    None
}

/// Run command on the host
pub fn system(cmd: &CStr) -> usize {
    let args = [cmd.as_ptr() as usize, cmd.count_bytes()];
    semihosting_call(OpCode::System, args.as_ptr() as usize).unwrap_or(usize::MAX)
}

/// Writes a character to the semihosting console.
pub fn writec(val: u8) {
    semihosting_call(OpCode::WriteC, &val as *const u8 as usize);
}

/// Semihosting console based on writec
pub struct Console {}

impl core::fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        s.as_bytes().iter().for_each(|v| writec(*v));
        Ok(())
    }
}

/// Print to semihosting console
#[macro_export]
macro_rules! print {
    ( $( $x:expr ),* $(,)? ) => {
        {
            use core::fmt::Write;
            write!($crate::Console{}, $($x,)*).unwrap();
        }
    };
}

/// Print line to semihosting console
#[macro_export]
macro_rules! println {
    ( $( $x:expr ),* $(,)? ) => {
        {
            $crate::print!($($x,)*);
            $crate::print!("\r\n");
        }
    };
}

/// Exits system.
pub fn shutdown(exit_code: usize) -> ! {
    let _ = semihosting_call(
        OpCode::ExitExtended,
        [ExitReason::ApplicationExit as usize, exit_code].as_ptr() as _,
    );
    loop {}
}

/// Mode for opening a file.
#[derive(Copy, Clone, Debug)]
#[repr(usize)]
pub enum OpenMode {
    /// read only.
    ReadOnly = 0,
    /// read, binary,
    ReadBinary = 1,
    /// read, write.
    ReadWrite = 2,
    /// read, write, binary.
    ReadWriteBinary = 3,
    /// write only.
    WriteOnly = 4,
    /// write binary.
    WriteBinary = 5,
    /// write, create if not exists.
    WriteCreate = 6,
    /// write, create, binary.
    WriteCreateBinary = 7,
    /// append only,
    Append = 8,
    /// append, binary,
    AppendBinary = 9,
    /// append, read
    AppendRead = 10,
    /// append, read, binary
    AppendReadBinary = 11,
}

/// Opens a file
pub fn fopen(path: &CStr, mode: OpenMode) -> Result<NonZeroUsize, Error> {
    let args: [usize; _] = [path.as_ptr() as _, mode as _, path.count_bytes()];
    match semihosting_call(OpCode::Open, args.as_ptr() as _).ok_or(Error::Unsupported)? {
        usize::MAX => Err(Error::Other(Some("fopen failed"))),
        v => NonZeroUsize::new(v).ok_or(Error::Other(Some("got zero handle"))),
    }
}

/// Closes a file
pub fn fclose(handle: NonZeroUsize) -> Result<(), Error> {
    let args: [usize; _] = [handle.get()];
    match semihosting_call(OpCode::Close, args.as_ptr() as _).ok_or(Error::Unsupported)? {
        0 => Ok(()),
        _ => Err(Error::Other(Some("fclose() failed"))),
    }
}

/// Reads data from a file
pub fn fread(handle: NonZeroUsize, out: &mut [u8]) -> Result<usize, Error> {
    let args: [usize; _] = [handle.get(), out.as_mut_ptr() as _, out.len()];
    match semihosting_call(OpCode::Read, args.as_ptr() as _).ok_or(Error::Unsupported)? {
        0 => Ok(out.len()),
        v if v >= out.len() => Err(Error::Other(Some("fread() failed"))),
        v => Ok(v),
    }
}

/// Writes data to a file
pub fn fwrite(handle: NonZeroUsize, data: &[u8]) -> Result<(), Error> {
    let args: [usize; _] = [handle.get(), data.as_ptr() as _, data.len()];
    match semihosting_call(OpCode::Write, args.as_ptr() as _).ok_or(Error::Unsupported)? {
        0 => Ok(()),
        _ => Err(Error::Other(Some("fwrite() failed"))),
    }
}

/// Queries file length
pub fn flen(handle: NonZeroUsize) -> Result<usize, Error> {
    let args: [usize; _] = [handle.get()];
    match semihosting_call(OpCode::FLen, args.as_ptr() as _).ok_or(Error::Unsupported)? {
        usize::MAX => Err(Error::Other(Some("flen() failed"))),
        v => Ok(v),
    }
}

/// Represent a file handle in semihosting context.
pub struct File(NonZeroUsize);

impl File {
    /// Open a file.
    pub fn open(path: &CStr, mode: OpenMode) -> Result<Self, Error> {
        Ok(Self(fopen(path, mode)?))
    }

    /// Writes data to it.
    pub fn write(&mut self, data: &[u8]) -> Result<(), Error> {
        fwrite(self.0, data)
    }

    /// Read data from it.
    pub fn read(&mut self, out: &mut [u8]) -> Result<usize, Error> {
        fread(self.0, out)
    }

    /// Queries file length.
    pub fn len(&self) -> Result<usize, Error> {
        flen(self.0)
    }
}

impl Drop for File {
    fn drop(&mut self) {
        let _ = fclose(self.0);
    }
}

/// Write the given data to a file
pub fn save_to_file(path: &CStr, data: &[u8]) -> Result<(), Error> {
    File::open(path, OpenMode::WriteCreate)?.write(data)
}
