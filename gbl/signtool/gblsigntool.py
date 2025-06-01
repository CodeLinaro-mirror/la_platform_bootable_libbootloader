#!/usr/bin/env python3
#
# Copyright 2025, The Android Open Source Project
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
#

"""Signs GBL images."""

from argparse import ArgumentParser
import glob
import hashlib
import os
import shlex
import shutil
import struct
import tempfile

from avbtool import (AvbFooter, AvbTool)


# Source of truth is winnt.h
IMAGE_DOS_SIGNATURE = 0x5A4D

IMAGE_NT_SIGNATURE = 0x4550
IMAGE_NT_OPTIONAL_HDR32_MAGIC = 0x010B
IMAGE_NT_OPTIONAL_HDR64_MAGIC = 0x020B

SIZEOF_IMAGE_DOS_HEADER = 64
SIZEOF_IMAGE_PE_HEADER = 24

OFFSET_E_LFANEW = 0x3C

OFFSET_PE_NUMBER_OF_SECTIONS = 6
OFFSET_PE_SIZE_OF_OPTIONAL_HEADER = 20

OFFSET_OPTIONAL_HDR_SIZE_OF_HEADERS = 60
OFFSET_OPTIONAL_HDR_CHECKSUM = 64
OFFSET_OPTIONAL_HDR32_NUMBER_OF_RVA_AND_SIZES = 92
OFFSET_OPTIONAL_HDR32_DATA_DIRECTORY = 96
OFFSET_OPTIONAL_HDR64_NUMBER_OF_RVA_AND_SIZES = 108
OFFSET_OPTIONAL_HDR64_DATA_DIRECTORY = 112

OFFSET_SECTION_HEADER_SIZE_OF_RAW_DATA = 16
OFFSET_SECTION_HEADER_POINTER_TO_RAW_DATA = 20

CERTIFICATE_TABLE_IDX = 4


def unpack_word(buf, off):
  """Unpacks a little-endian 2-byte word."""
  return struct.unpack_from('<H', buf, off)[0]


def unpack_int(buf, off):
  """Unpacks a little-endian 4-byte int."""
  return struct.unpack_from('<I', buf, off)[0]


class PEError(ValueError):
  """PE file parsing related errors."""


class DOSHeader:
  """Helper class for DOS header."""

  def __init__(self, buf):
    self._buf = buf

    if len(self._buf) < SIZEOF_IMAGE_DOS_HEADER:
      raise PEError('Image size is too small for a DOS header')

    magic = unpack_word(self._buf, 0)
    if magic != IMAGE_DOS_SIGNATURE:
      raise PEError(f'Unexpected DOS magic: 0x{magic:04X}')

  def e_lfanew(self):
    return unpack_word(self._buf, OFFSET_E_LFANEW)


class PEHeader:
  """Helper class for PE header."""

  def __init__(self, buf):
    self._buf = buf

    if len(self._buf) < SIZEOF_IMAGE_PE_HEADER:
      raise PEError('Image size is too small for a PE header')

    magic = unpack_int(self._buf, 0)
    if magic != IMAGE_NT_SIGNATURE:
      raise PEError(f'Unexpected PE magic: 0x{magic:08X}')

  def size_of_optional_header(self):
    return unpack_word(self._buf, OFFSET_PE_SIZE_OF_OPTIONAL_HEADER)

  def number_of_sections(self):
    return unpack_word(self._buf, OFFSET_PE_NUMBER_OF_SECTIONS)


class OptionalHeader:
  """Helper class for PE32/PE32+ Optional header."""

  def __init__(self, buf):
    self._buf = buf

    magic = unpack_word(self._buf, 0)
    if magic == IMAGE_NT_OPTIONAL_HDR32_MAGIC:
      self._pe_plus = False
    elif magic == IMAGE_NT_OPTIONAL_HDR64_MAGIC:
      self._pe_plus = True
    else:
      raise PEError(f'Unexpected PE Optional header magic: 0x{magic:04X}')

  def number_of_data_entries(self):
    if self._pe_plus:
      offset = OFFSET_OPTIONAL_HDR64_NUMBER_OF_RVA_AND_SIZES
    else:
      offset = OFFSET_OPTIONAL_HDR32_NUMBER_OF_RVA_AND_SIZES
    return unpack_int(self._buf, offset)

  def data_entry_offset(self, idx):
    if self._pe_plus:
      return OFFSET_OPTIONAL_HDR64_DATA_DIRECTORY + idx * 8
    else:
      return OFFSET_OPTIONAL_HDR32_DATA_DIRECTORY + idx * 8

  def data_entry(self, idx):
    return struct.unpack_from('<II', self._buf, self.data_entry_offset(idx))

  def size_of_headers(self):
    return unpack_int(self._buf, OFFSET_OPTIONAL_HDR_SIZE_OF_HEADERS)


class PEImage:
  """PE file parser."""

  def __init__(self, buf):
    self._buf = bytearray(buf)

    self._dos_header = DOSHeader(self._buf)
    self._pe_header_offset = self._dos_header.e_lfanew()
    self._optional_header_offset = (
        self._pe_header_offset + SIZEOF_IMAGE_PE_HEADER
    )
    self._pe_header = PEHeader(self._buf[self._pe_header_offset :])
    self._checksum_offset = (
        self._optional_header_offset + OFFSET_OPTIONAL_HDR_CHECKSUM
    )
    self._section_headers_offset = (
        self._optional_header_offset + self._pe_header.size_of_optional_header()
    )
    self._optional_header = OptionalHeader(
        self._buf[self._optional_header_offset :]
    )
    if self._optional_header.number_of_data_entries() < 5:
      raise PEError('PE Optional header data directories table is too small')

  def erase_existing_win_certificates(self):
    offset, size = self._optional_header.data_entry(CERTIFICATE_TABLE_IDX)
    certificate_table_offset = (
        self._optional_header_offset
        + self._optional_header.data_entry_offset(CERTIFICATE_TABLE_IDX)
    )
    self._buf[certificate_table_offset : certificate_table_offset + 8] = (
        8 * b'\x00'
    )
    if offset or size:
      if offset > len(self._buf):
        print('WARNING: certificate table offset is past EOF')
        return
      if offset + size != len(self._buf):
        print('WARNING: removed junk data after the certificate table')
      self._buf = self._buf[:offset]
      print('Erased existing SecureBoot certificates')

  def erase_checksum(self):
    self._buf[self._checksum_offset : self._checksum_offset + 4] = 4 * b'\x00'

  def get_avb_footer(self):
    if len(self._buf) >= AvbFooter.SIZE:
      try:
        return AvbFooter(self._buf[-AvbFooter.SIZE :])
      except (LookupError, struct.error):
        pass
    return None

  def authenticode_digest(self):
    data_directory_certificate_table_offset = (
        self._optional_header_offset
        + self._optional_header.data_entry_offset(CERTIFICATE_TABLE_IDX)
    )
    data_directory_certificate_table_end = (
        self._optional_header_offset
        + self._optional_header.data_entry_offset(CERTIFICATE_TABLE_IDX + 1)
    )
    regions = [
        (0, self._checksum_offset),
        (self._checksum_offset + 4, data_directory_certificate_table_offset),
        (
            data_directory_certificate_table_end,
            self._optional_header.size_of_headers(),
        ),
    ]
    for idx in range(self._pe_header.number_of_sections()):
      off = self._section_headers_offset + idx * 40
      size = unpack_int(self._buf, off + OFFSET_SECTION_HEADER_SIZE_OF_RAW_DATA)
      data = unpack_int(
          self._buf, off + OFFSET_SECTION_HEADER_POINTER_TO_RAW_DATA
      )
      regions.append((data, data + size))
    regions.sort(key=lambda e: e[0])

    # End junk
    regions.append((regions[-1][1], len(self._buf)))

    hasher = hashlib.sha256()
    for begin, end in regions:
      hasher.update(self._buf[begin:end])
    return hasher.hexdigest()


def gbl_info(args):
  """Shows info about a GBL image."""
  with open(args.gbl_image, 'rb') as gbl:
    gbl_bytes = gbl.read()
  gbl_image = PEImage(gbl_bytes)
  gbl_image.erase_existing_win_certificates()
  gbl_image.erase_checksum()
  print('Authenticode digest (sha256):', gbl_image.authenticode_digest())

  avb_footer = gbl_image.get_avb_footer()
  if not avb_footer:
    raise ValueError('No AVB footer found, image is unsigned')

  with tempfile.TemporaryDirectory() as temp_dir:
    gbl_efi = os.path.join(temp_dir, 'gbl.efi')
    with open(gbl_efi, 'wb') as f:
      f.write(gbl_image._buf)
    gbl_image._buf = gbl_image._buf[: avb_footer.original_image_size]
    print(
        'Authenticode digest (without AVB footer):',
        gbl_image.authenticode_digest(),
    )
    print('====VBMETA====')
    AvbTool().run(['avbtool', 'info_image', '--image', gbl_efi])


def gbl_sign_one(gbl_image, output, avbtool_args):
  """Signs one GBL image."""
  with open(gbl_image, 'rb') as gbl:
    gbl_bytes = gbl.read()
  gbl_image = PEImage(gbl_bytes)
  gbl_image.erase_existing_win_certificates()
  gbl_image.erase_checksum()
  avb_footer = gbl_image.get_avb_footer()
  if avb_footer:
    gbl_image._buf = gbl_image._buf[: avb_footer.original_image_size]
    print('Erased existing AVB footer')
  digest = gbl_image.authenticode_digest()

  with tempfile.TemporaryDirectory() as temp_dir:
    gbl_efi = os.path.join(temp_dir, 'gbl.efi')
    with open(gbl_efi, 'wb') as f:
      f.write(gbl_image._buf)
    avb_cmd = (
        [
            'avbtool',
            'add_hash_footer',
        ]
        + avbtool_args
        + [
            '--image',
            gbl_efi,
            '--partition_name',
            'gbl',
            '--dynamic_partition_size',
            '--prop',
            f'authenticode:{digest}',
        ]
    )
    print('avbtool command:', ' '.join(avb_cmd))
    AvbTool().run(avb_cmd)
    print(f'Authenticode digest (sha256): {digest}')
    shutil.move(gbl_efi, output)
    print(f'Signed image written to {output}')


def gbl_sign(args):
  """Signs a GBL image."""
  gbl_sign_one(args.gbl_image, args.output, args.avbtool_args)


def gbl_sign_archive(args):
  """Signs a GBL image archive."""
  with tempfile.TemporaryDirectory() as temp_dir:
    unpack_dir = os.path.join(temp_dir, 'unpack')
    shutil.unpack_archive(args.image_archive, unpack_dir)
    signed_dir = os.path.join(temp_dir, 'signed')
    os.mkdir(signed_dir)
    for gbl_efi in glob.glob(os.path.join(unpack_dir, 'gbl*.efi')):
      stem = os.path.basename(gbl_efi)
      print(f'Found GBL image: {stem}')
      gbl_sign_one(gbl_efi, os.path.join(signed_dir, stem), args.avbtool_args)
      print('')

    archive_name = shutil.make_archive(
        os.path.join(temp_dir, 'signed_zip'), 'zip', signed_dir
    )
    shutil.move(archive_name, args.output)
    print(f'Signed image archive written to {args.output}')


def gbl_verify(args):
  """Verifies a signed GBL image."""
  with open(args.gbl_image, 'rb') as gbl:
    gbl_bytes = gbl.read()
  gbl_image = PEImage(gbl_bytes)
  gbl_image.erase_existing_win_certificates()
  gbl_image.erase_checksum()

  with tempfile.TemporaryDirectory() as temp_dir:
    gbl_efi = os.path.join(temp_dir, 'gbl.efi')
    with open(gbl_efi, 'wb') as f:
      f.write(gbl_image._buf)
    avb_cmd = [
        'avbtool',
        'verify_image',
        '--image',
        gbl_efi,
    ]
    if args.key:
      avb_cmd += ['--key', args.key]
    AvbTool().run(avb_cmd)


def gbl_remove(args):
  """Removes signatures."""
  with open(args.gbl_image, 'rb') as gbl:
    gbl_bytes = gbl.read()
  gbl_image = PEImage(gbl_bytes)
  gbl_image.erase_existing_win_certificates()
  gbl_image.erase_checksum()
  avb_footer = gbl_image.get_avb_footer()
  if avb_footer:
    gbl_image._buf = gbl_image._buf[: avb_footer.original_image_size]
    print('Erased existing AVB footer')
  with open(args.output, 'wb') as f:
    f.write(gbl_image._buf)


def flatten_args(raw_args):
  """Split and flatten nested args."""
  args = []
  for a in raw_args:
    args.extend(shlex.split(a))
  return args


def main():
  parser = ArgumentParser()
  subcommands = parser.add_subparsers(required=True, title='subcommands')

  info_command = subcommands.add_parser(
      'info', help='show info about a GBL image'
  )
  info_command.add_argument(
      'gbl_image', metavar='GBL_IMAGE', help='GBL EFI image'
  )
  info_command.set_defaults(func=gbl_info)

  sign_command = subcommands.add_parser('sign', help='sign a GBL image')
  sign_command.add_argument(
      'gbl_image', metavar='GBL_IMAGE', help='GBL EFI image'
  )
  sign_command.add_argument(
      '-o', '--output', required=True, help='output file name'
  )
  sign_command.add_argument(
      '--avbtool_args',
      default=[],
      action='append',
      help='signing args to pass to avbtool (can be specified multiple times)',
  )
  sign_command.set_defaults(func=gbl_sign)

  sign_archive_command = subcommands.add_parser(
      'sign_archive', help='sign a GBL image archive'
  )
  sign_archive_command.add_argument(
      'image_archive', metavar='IMAGE_ARCHIVE', help='zip archive of GBL images'
  )
  sign_archive_command.add_argument(
      '-o', '--output', required=True, help='output archive name'
  )
  sign_archive_command.add_argument(
      '--avbtool_args',
      default=[],
      action='append',
      help='signing args to pass to avbtool (can be specified multiple times)',
  )
  sign_archive_command.set_defaults(func=gbl_sign_archive)

  verify_command = subcommands.add_parser(
      'verify', help='verify a signed GBL image'
  )
  verify_command.add_argument(
      'gbl_image', metavar='GBL_IMAGE', help='GBL EFI image'
  )
  verify_command.add_argument('--key', help='check public key')
  verify_command.set_defaults(func=gbl_verify)

  remove_command = subcommands.add_parser(
      'remove', help='remove any signatures from a GBL image'
  )
  remove_command.add_argument(
      'gbl_image', metavar='GBL_IMAGE', help='GBL EFI image'
  )
  remove_command.add_argument(
      '-o', '--output', required=True, help='output file name'
  )
  remove_command.set_defaults(func=gbl_remove)

  args = parser.parse_args()
  if 'avbtool_args' in args:
    args.avbtool_args = flatten_args(args.avbtool_args)

  return args.func(args)


if __name__ == '__main__':
  main()
