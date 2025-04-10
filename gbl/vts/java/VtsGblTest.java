/*
 * Copyright (C) 2025 The Android Open Source Project
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

package com.android.tests.gbl;

import static org.hamcrest.CoreMatchers.not;
import static org.hamcrest.Matchers.matchesPattern;
import static org.junit.Assert.assertNotNull;
import static org.junit.Assert.assertThat;
import static org.junit.Assume.assumeThat;

import com.android.tradefed.device.DeviceNotAvailableException;
import com.android.tradefed.device.ITestDevice;
import com.android.tradefed.log.LogUtil.CLog;
import com.android.tradefed.testtype.DeviceJUnit4ClassRunner;
import com.android.tradefed.testtype.junit4.BaseHostJUnit4Test;
import java.io.IOException;
import org.junit.Before;
import org.junit.Test;
import org.junit.runner.RunWith;

@RunWith(DeviceJUnit4ClassRunner.class)
public class VtsGblTest extends BaseHostJUnit4Test {
  @Before
  public final void setUp() throws DeviceNotAvailableException, IOException {
    ITestDevice device = getDevice();
    final long gblVersion = device.getIntProperty("ro.boot.gbl.version", -1L);
    assumeThat("GBL version prop", gblVersion, not(-1L));
  }

  @Test
  public void testSystemProperties() throws DeviceNotAvailableException, NumberFormatException {
    ITestDevice device = getDevice();
    final long gblVersion = device.getIntProperty("ro.boot.gbl.version", -1);
    final String gblBuildNumber = device.getProperty("ro.boot.gbl.build_number");

    CLog.i("GBL version: " + gblVersion);
    CLog.i("GBL build_number: " + gblBuildNumber);

    assertNotNull(gblBuildNumber);

    if (gblBuildNumber.startsWith("eng.")) {
      CLog.w("GBL is a local eng build");
    }

    assertThat("Invalid build ID", gblBuildNumber, matchesPattern("P?[0-9]+"));

    if (gblBuildNumber.startsWith("P")) {
      CLog.i("Skipping rest of test because GBL is presubmit build");
      return;
    }
    final long gblBuildIncremental = Long.parseLong(gblBuildNumber);
  }
}
