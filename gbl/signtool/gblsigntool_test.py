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


from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


def GetResource(*args):
  return Path(sys.argv[0]).absolute().parent.joinpath(*args)


def Gblsigntool(*args):
  return subprocess.run([GetResource('gblsigntool')] + list(args), check=True)


TEST_KEY_PATH = GetResource('testdata', 'testkey_RSA4096.pem')
TEST_PUBKEY_PATH = GetResource('testdata', 'testkey_RSA4096_pub.pem')

# These are originally testkey-signed by another key, _not_ the above key.
TEST_EFI_PATH = GetResource('testdata', 'gbl_aarch64_prod.efi')
TEST_IMG_ZIP_PATH = GetResource('testdata', 'gbl-img-13454081.zip')


class GblSigntoolTest(unittest.TestCase):

  def testInfo_success(self):
    Gblsigntool('info', TEST_EFI_PATH)

  def testInfo_notPEImage_failure(self):
    with tempfile.TemporaryDirectory() as temp_dir:
      regular_file = Path(temp_dir).joinpath('regular_file')
      with open(regular_file, 'wb') as f:
        f.write(b'blah' * 1000)
      with self.assertRaises(subprocess.CalledProcessError):
        Gblsigntool('info', regular_file)

  def testInfo_unsigned_failure(self):
    with tempfile.TemporaryDirectory() as temp_dir:
      unsigned_efi = Path(temp_dir).joinpath('unsigned.efi')
      Gblsigntool('remove', TEST_EFI_PATH, '-o', unsigned_efi)
      with self.assertRaises(subprocess.CalledProcessError):
        Gblsigntool('info', unsigned_efi)

  def testRemove_idempotent(self):
    with tempfile.TemporaryDirectory() as temp_dir:
      unsigned_efi = Path(temp_dir).joinpath('unsigned.efi')
      Gblsigntool('remove', TEST_EFI_PATH, '-o', unsigned_efi)
      unsigned_twice_efi = Path(temp_dir).joinpath('unsigned_twice.efi')
      Gblsigntool('remove', unsigned_efi, '-o', unsigned_twice_efi)
      with open(unsigned_efi, 'rb') as f:
        unsigned_bytes = f.read()
      with open(unsigned_twice_efi, 'rb') as f:
        unsigned_twice_bytes = f.read()
      self.assertEqual(unsigned_bytes, unsigned_twice_bytes)

  def testVerify_success(self):
    Gblsigntool('verify', TEST_EFI_PATH)

  def testVerify_pubkeyMismatch_failure(self):
    with self.assertRaises(subprocess.CalledProcessError):
      Gblsigntool('verify', TEST_EFI_PATH, '--key', TEST_PUBKEY_PATH)

  def testVerify_notPEImage_failure(self):
    with tempfile.TemporaryDirectory() as temp_dir:
      regular_file = Path(temp_dir).joinpath('regular_file')
      with open(regular_file, 'wb') as f:
        f.write(b'blah' * 1000)
      with self.assertRaises(subprocess.CalledProcessError):
        Gblsigntool('verify', regular_file)

  def testVerify_unsigned_failure(self):
    with tempfile.TemporaryDirectory() as temp_dir:
      unsigned_efi = Path(temp_dir).joinpath('unsigned.efi')
      Gblsigntool('remove', TEST_EFI_PATH, '-o', unsigned_efi)
      with self.assertRaises(subprocess.CalledProcessError):
        Gblsigntool('verify', unsigned_efi)

  def testSign_success(self):
    with tempfile.TemporaryDirectory() as temp_dir:
      unsigned_efi = Path(temp_dir).joinpath('unsigned.efi')
      Gblsigntool('remove', TEST_EFI_PATH, '-o', unsigned_efi)
      signed_efi = Path(temp_dir).joinpath('signed.efi')
      Gblsigntool(
          'sign',
          unsigned_efi,
          '-o',
          signed_efi,
          '--avbtool_args',
          f'--key "{TEST_KEY_PATH}" --algorithm SHA256_RSA4096',
      )
      Gblsigntool('verify', signed_efi, '--key', TEST_PUBKEY_PATH)

  def testSign_resign_success(self):
    with tempfile.TemporaryDirectory() as temp_dir:
      signed_efi = Path(temp_dir).joinpath('signed.efi')
      Gblsigntool(
          'sign',
          TEST_EFI_PATH,
          '-o',
          signed_efi,
          '--avbtool_args',
          f'--key "{TEST_KEY_PATH}" --algorithm SHA256_RSA4096',
      )
      Gblsigntool('verify', signed_efi, '--key', TEST_PUBKEY_PATH)

  def testSignArchive_success(self):
    with tempfile.TemporaryDirectory() as temp_dir:
      unpack_dir = Path(temp_dir).joinpath('unpack')
      shutil.unpack_archive(TEST_IMG_ZIP_PATH, unpack_dir)
      for efi in unpack_dir.glob('gbl*.efi'):
        with self.assertRaises(subprocess.CalledProcessError):
          Gblsigntool('verify', efi, '--key', TEST_PUBKEY_PATH)

      signed_zip = Path(temp_dir).joinpath('signed.zip')
      Gblsigntool(
          'sign_archive',
          TEST_IMG_ZIP_PATH,
          '-o',
          signed_zip,
          '--avbtool_args',
          f'--key "{TEST_KEY_PATH}" --algorithm SHA256_RSA4096',
      )
      unpack_signed_dir = Path(temp_dir).joinpath('unpack_signed')
      shutil.unpack_archive(signed_zip, unpack_signed_dir)
      for signed_efi in unpack_signed_dir.glob('gbl*.efi'):
        Gblsigntool('verify', signed_efi, '--key', TEST_PUBKEY_PATH)


if __name__ == '__main__':
  unittest.main(verbosity=2)
