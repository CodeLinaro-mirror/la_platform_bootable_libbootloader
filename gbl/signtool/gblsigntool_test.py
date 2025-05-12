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

import glob
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

EXEC_DIR = os.path.abspath(os.path.dirname(sys.argv[0]))


def get_resource(*args):
  return os.path.join(EXEC_DIR, *args)


class GblSigntoolTest(unittest.TestCase):

  def setUp(self):
    self._gblsigntool = get_resource('gblsigntool')
    self._test_key = get_resource('testkeys', 'testkey_RSA4096.pem')
    self._test_pubkey = get_resource('testkeys', 'testkey_RSA4096_pub.pem')
    self._test_efi = get_resource('testdata', 'gbl_aarch64_prod.efi')
    self._test_img_zip = get_resource('testdata', 'gbl-img.zip')

  def testVerify_withUnsignedInput(self):
    result = subprocess.run([
        self._gblsigntool,
        'verify',
        self._test_efi,
        '--key',
        self._test_pubkey,
    ])
    self.assertNotEqual(result.returncode, 0)

  def testSignAndVerify(self):
    with tempfile.TemporaryDirectory() as temp_dir:
      signed_efi = os.path.join(temp_dir, 'signed.img')
      subprocess.run(
          [
              self._gblsigntool,
              'sign',
              self._test_efi,
              '-o',
              signed_efi,
              '--avbtool_args',
              f'--key "{self._test_key}" --algorithm SHA256_RSA4096',
          ],
          check=True,
      )
      subprocess.run(
          [self._gblsigntool, 'verify', signed_efi, '--key', self._test_pubkey],
          check=True,
      )

  def testSignArchiveAndVerify(self):
    with tempfile.TemporaryDirectory() as temp_dir:
      signed_zip = os.path.join(temp_dir, 'signed.zip')
      subprocess.run(
          [
              self._gblsigntool,
              'sign_archive',
              self._test_img_zip,
              '-o',
              signed_zip,
              '--avbtool_args',
              f'--key "{self._test_key}" --algorithm SHA256_RSA4096',
          ],
          check=True,
      )
      unpack_dir = os.path.join(temp_dir, 'signed')
      shutil.unpack_archive(signed_zip, unpack_dir)
      for signed_efi in glob.glob(os.path.join(unpack_dir, 'gbl*.efi')):
        subprocess.run(
            [
                self._gblsigntool,
                'verify',
                signed_efi,
                '--key',
                self._test_pubkey,
            ],
            check=True,
        )


if __name__ == '__main__':
  unittest.main()
