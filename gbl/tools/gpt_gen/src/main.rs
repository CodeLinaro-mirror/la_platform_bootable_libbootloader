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

//! `gpt_gen` is a host tool for generating GPT (GUID Partition Table) disk images.
//!
//! It allows users to specify the disk size, partition table entries count, and
//! details of each partition (name, size, and optional file content).
//!
//! # Example Usage
//!
//! ```sh
//! ./bazel.sh run //bootable/libbootloader/gbl/tools/gpt_gen -- \
//!     --out disk.img --disk_size 1G \
//!     --partition="boot,32M,boot.img" \
//!     --partition="system,512M"
//! ```

use clap::Parser;

/// Command line arguments for `gpt_gen`.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Output file
    out: String,

    /// Disk size of the image. e.g. 1G, 512M, 1024K
    disk_size: String,

    /// Specifies a partition. Format should be --partition=<part name>,<size>,<file name>
    #[arg(long, action = clap::ArgAction::Append)]
    partition: Vec<String>,

    /// Number of entries in partition table. i.e. 128, 256
    #[arg(long = "entries_count", default_value_t = 128)]
    entries_count: u32,
}

/// Parses a size string with optional unit suffix (K, M) into bytes.
///
/// Supported suffixes:
/// - `k` or `K` for Kilobytes (1024 bytes)
/// - `m` or `M` for Megabytes (1024 * 1024 bytes)
/// - No suffix for bytes
fn parse_size_str(size_str: &str) -> Result<u64, String> {
    let size_str = size_str.to_lowercase();
    let (value_str, multiplier) = match size_str.chars().last() {
        Some('k') => (&size_str[..size_str.len() - 1], 1024),
        Some('m') => (&size_str[..size_str.len() - 1], 1024 * 1024),
        _ => (size_str.as_str(), 1),
    };
    let val = value_str.parse::<u64>().map_err(|e| e.to_string())?;
    Ok(val * multiplier)
}

fn main() {
    let args = Args::parse();
    println!("Output file: {}", args.out);
    println!("Disk size: {}", args.disk_size);
    println!("Partitions: {:?}", args.partition);

    let size = parse_size_str(&args.disk_size).unwrap();
    assert!(size % 512 == 0, "Disk size must be a multiple of 512");
    unimplemented!()
}
