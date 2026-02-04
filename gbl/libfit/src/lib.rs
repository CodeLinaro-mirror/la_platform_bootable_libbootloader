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

//! This library provides APIs to work with FIT image.
//!
//! Reference documentation:
//! https://cs.android.com/android/kernel/superproject/+/common-android-mainline:bootable/libbootloader/gbl/docs/gbl_fit.md

#![cfg_attr(not(test), no_std)]

use arrayvec::{ArrayString, ArrayVec};
use core::{ffi::CStr, fmt::Write};
use fdt::{Fdt, FdtHeader, MAXIMUM_OVERLAYS_TO_APPLY};
use liberror::{Error, Result};
use safemath::SafeNum;

/// Standard FIT property names
const TYPE: &CStr = c"type";
const FDT: &CStr = c"fdt";
const DATA: &CStr = c"data";
const DATA_OFFSET: &CStr = c"data-offset";
const DATA_SIZE: &CStr = c"data-size";
const DEFAULT: &CStr = c"default";

/// Standard FIT node paths
const CONFIGURATIONS: &str = "/configurations";
const IMAGES: &str = "/images";

/// Structure for FIT partition buffer
pub struct Fit<'a> {
    fdt: Fdt<&'a [u8]>,
    external_images: &'a [u8],
}

impl<'a> Fit<'a> {
    /// Creates Fit wrapping the contents of the buffer
    pub fn from_bytes(buffer: &'a [u8]) -> Result<Self> {
        let header = FdtHeader::from_bytes_ref(buffer)?;
        let (fdt_buffer, image_buffer) = buffer.split_at(header.totalsize());
        Ok(Self { fdt: Fdt::new(fdt_buffer)?, external_images: image_buffer })
    }

    /// Returns FIT FDT buffer and metadata buffer when reference to metadata
    /// payload is present in the FIT FDT
    pub fn get_fit_selection_metadata(&self) -> Result<(&[u8], Option<&[u8]>)> {
        // If present, reference to the metadata must be present in the first sub-node under
        // "images" node
        let first_images_subnode =
            self.fdt.get_first_subnode_offset(self.fdt.find_node_offset(IMAGES)?)?;
        let type_property = CStr::from_bytes_with_nul(
            self.fdt.get_property_by_node_offset(first_images_subnode, TYPE)?,
        )?
        .to_str()?;

        // Metadata sub-node must contain "type" property which is set to "metadata"
        let selected_metadata = match type_property {
            "metadata" => Some(self.get_image_at_offset(first_images_subnode)?),
            _ => None,
        };

        Ok((self.fdt.as_ref(), selected_metadata))
    }

    /// Get base and overlay devicetrees corresponding to the selected configuration.
    /// Fallback to default configuration in case no configuration is selected.
    pub fn get_devicetrees_from_selected_configuration(
        &'a self,
        selected_offset: Option<usize>,
    ) -> Result<(&'a [u8], ArrayVec<&'a [u8], MAXIMUM_OVERLAYS_TO_APPLY>)> {
        let mut base: Option<&[u8]> = None;
        let mut overlays: ArrayVec<&[u8], MAXIMUM_OVERLAYS_TO_APPLY> = ArrayVec::new();
        let selected_config = match selected_offset {
            Some(conifguration_offset) => conifguration_offset,
            _ => self.get_default_configuration_offset()?,
        };

        let fdt_iter = self.fdt.get_property_stringlist_by_node_offset(selected_config, FDT)?;
        for (idx, fdt_str) in fdt_iter.enumerate() {
            let mut fdt_path: ArrayString<32> = ArrayString::new();
            write!(fdt_path, "{IMAGES}/{}", fdt_str.to_str()?)?;
            let fdt_image = self.get_image_by_node_path(fdt_path.as_str())?;

            // The first FDT refers to the base devicetree and the remaining FDTs
            // refer to overlay devicetrees.
            // https://fitspec.osfw.foundation/#optional-properties
            if idx == 0 {
                base = Some(fdt_image);
                continue;
            }

            overlays.push(fdt_image);
        }

        Ok((base.unwrap(), overlays))
    }

    /// Return offset for FIT default configuration node in FIT FDT
    fn get_default_configuration_offset(&self) -> Result<usize> {
        let default_string =
            CStr::from_bytes_with_nul(self.fdt.get_property(CONFIGURATIONS, DEFAULT)?)?.to_str()?;

        let mut default_configuration_path: ArrayString<32> = ArrayString::new();
        write!(default_configuration_path, "{CONFIGURATIONS}/{default_string}")?;

        self.fdt
            .find_node_offset(default_configuration_path.as_str())
            .map(|e| e.try_into().unwrap())
    }

    /// Return image offset in FIT FDT
    fn get_image_by_node_path(&'a self, path: &str) -> Result<&'a [u8]> {
        self.get_image_at_offset(self.fdt.find_node_offset(path)?)
    }

    /// Return image buffer given the offset of the image node in FIT FDT
    fn get_image_at_offset(&'a self, node_offset: usize) -> Result<&'a [u8]> {
        match self.fdt.get_property_by_node_offset(node_offset, DATA) {
            Ok(embedded_image) => Ok(embedded_image),
            _ => {
                // Get image offset wrt the position from FIT FDT
                let image_offset = SafeNum::from(u32::from_be_bytes(
                    self.fdt
                        .get_property_by_node_offset(node_offset, DATA_OFFSET)?
                        .try_into()
                        .map_err(|_| Error::Other(Some("not a u32 value")))?,
                ));

                let image_size = u32::from_be_bytes(
                    self.fdt
                        .get_property_by_node_offset(node_offset, DATA_SIZE)?
                        .try_into()
                        .map_err(|_| Error::Other(Some("not a u32 value")))?,
                );

                self.external_images
                    .get(image_offset.try_into()?..(image_offset + image_size).try_into()?)
                    .ok_or(Error::Other(Some("Invalid image slice bounds")))
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_read_fit_image() {
        let fit_buf = include_bytes!("../test/data/fit.img").to_vec();
        let fit_buf = fit_buf.as_slice();

        const FIT_SIZE: usize = 1304;
        let (_, payload_buf) = fit_buf.split_at(FIT_SIZE);

        let fit_image = Fit::from_bytes(fit_buf).unwrap();
        let fit_header = fit_image.fdt.header_ref();
        let fit_size = fit_header.unwrap().totalsize();

        assert_eq!(payload_buf, fit_image.external_images);
        assert_eq!(
            FdtHeader::from_bytes_ref(payload_buf),
            FdtHeader::from_bytes_ref(fit_image.external_images)
        );
        assert_eq!(fit_size, FIT_SIZE);
    }

    #[test]
    fn test_invalid_fit_image() {
        let fit_buf = include_bytes!("../test/data/zeros.img").to_vec();
        let fit_buf = fit_buf.as_slice();

        assert!(Fit::from_bytes(fit_buf).is_err());
    }

    #[test]
    fn test_get_fit_selection_metadata() {
        let fit_buf = include_bytes!("../test/data/fit.img").to_vec();
        let fit_buf = fit_buf.as_slice();

        let fit_image = Fit::from_bytes(fit_buf).unwrap();
        let (_, metadata) = fit_image.get_fit_selection_metadata().unwrap();

        assert!(metadata.is_some());
        assert!(!metadata.unwrap().is_empty());
    }

    #[test]
    fn test_fit_with_no_metadata() {
        let fit_buf = include_bytes!("../test/data/fit_with_no_metadata.img").to_vec();
        let fit_buf = fit_buf.as_slice();

        let fit_image = Fit::from_bytes(fit_buf).unwrap();
        let (_, metadata) = fit_image.get_fit_selection_metadata().unwrap();

        assert!(metadata.is_none());
    }

    #[test]
    fn test_fit_with_invalid_metadata_type() {
        let fit_buf = include_bytes!("../test/data/fit_with_invalid_metadata_type.img").to_vec();
        let fit_buf = fit_buf.as_slice();

        let fit_image = Fit::from_bytes(fit_buf).unwrap();
        let (_, metadata) = fit_image.get_fit_selection_metadata().unwrap();

        assert!(metadata.is_none());
    }

    #[test]
    fn test_fit_with_invalid_metadata_position() {
        let fit_buf =
            include_bytes!("../test/data/fit_with_invalid_metadata_position.img").to_vec();
        let fit_buf = fit_buf.as_slice();

        let fit_image = Fit::from_bytes(fit_buf).unwrap();
        let (_, metadata) = fit_image.get_fit_selection_metadata().unwrap();

        assert!(metadata.is_none());
    }

    #[test]
    fn test_default_configuration() {
        let fit_buf = include_bytes!("../test/data/fit_with_default_option.img").to_vec();
        let fit_buf = fit_buf.as_slice();

        let fit_image = Fit::from_bytes(fit_buf).unwrap();
        let default_configuration_offset = fit_image.get_default_configuration_offset().unwrap();

        assert_eq!(
            CStr::from_bytes_with_nul(
                fit_image
                    .fdt
                    .get_property_by_node_offset(
                        default_configuration_offset.try_into().unwrap(),
                        c"description"
                    )
                    .unwrap()
            )
            .unwrap()
            .to_str()
            .unwrap(),
            "test config-1"
        );
    }

    #[test]
    fn test_no_default_configuration() {
        let fit_buf = include_bytes!("../test/data/fit.img").to_vec();
        let fit_buf = fit_buf.as_slice();

        let fit_image = Fit::from_bytes(fit_buf).unwrap();
        assert!(fit_image.get_default_configuration_offset().is_err());
    }

    #[test]
    fn test_get_devicetrees_from_selected_configuration_only_base() {
        let fit_buf = include_bytes!("../test/data/fit.img").to_vec();
        let fit_buf = fit_buf.as_slice();

        let fit_image = Fit::from_bytes(fit_buf).unwrap();

        let (base, overlays) = fit_image
            .get_devicetrees_from_selected_configuration(Some(
                fit_image.fdt.find_node_offset("/configurations/config-1").unwrap(),
            ))
            .unwrap();

        let base_fdt_data = include_bytes!("../test/data/platform-1.dtb").to_vec();
        let base_fdt_buffer = base_fdt_data.as_slice();

        assert!(overlays.is_empty());
        assert_eq!(base_fdt_buffer, base);
    }

    #[test]
    fn test_get_devicetrees_from_selected_configuration() {
        let fit_buf = include_bytes!("../test/data/fit.img").to_vec();
        let fit_buf = fit_buf.as_slice();
        let fit_image = Fit::from_bytes(fit_buf).unwrap();

        let (base, overlays) = fit_image
            .get_devicetrees_from_selected_configuration(Some(
                fit_image.fdt.find_node_offset("/configurations/config-5").unwrap(),
            ))
            .unwrap();

        let base_fdt_data = include_bytes!("../test/data/platform-1.dtb").to_vec();
        let base_fdt_buffer = base_fdt_data.as_slice();

        let overlay1_fdt_data = include_bytes!("../test/data/overlay-1.dtb").to_vec();
        let overlay1_fdt_buffer = overlay1_fdt_data.as_slice();

        let overlay2_fdt_data = include_bytes!("../test/data/overlay-2.dtb").to_vec();
        let overlay2_fdt_buffer = overlay2_fdt_data.as_slice();

        let mut overlay_fdt: ArrayVec<&[u8], MAXIMUM_OVERLAYS_TO_APPLY> = ArrayVec::new();
        overlay_fdt.push(overlay1_fdt_buffer);
        overlay_fdt.push(overlay2_fdt_buffer);

        assert!(!overlays.is_empty());
        assert_eq!(base_fdt_buffer, base);
        assert_eq!(overlay_fdt, overlays);
    }

    #[test]
    fn test_get_devicetrees_from_selected_configuration_invalid() {
        let fit_buf = include_bytes!("../test/data/fit.img").to_vec();
        let fit_buf = fit_buf.as_slice();
        let fit_image = Fit::from_bytes(fit_buf).unwrap();

        let result = fit_image.get_devicetrees_from_selected_configuration(Some(
            fit_image.fdt.find_node_offset("/configurations/config-6").unwrap(),
        ));

        assert!(result.is_err());
    }

    #[test]
    fn test_get_image_by_node_path_embedded_payload() {
        let fit_buf = include_bytes!("../test/data/fit_embedded_payload.img").to_vec();
        let fit_buf = fit_buf.as_slice();

        let fit_image = Fit::from_bytes(fit_buf).unwrap();
        let fdt_image_from_fit = fit_image.get_image_by_node_path("/images/fdt-2").unwrap();

        let fdt2_image_data = include_bytes!("../test/data/platform-2.dtb").to_vec();
        let fdt2_image_buffer = fdt2_image_data.as_slice();

        assert_eq!(fdt2_image_buffer, fdt_image_from_fit);
    }

    #[test]
    fn test_get_image_by_node_path_external_payload() {
        let fit_buf = include_bytes!("../test/data/fit.img").to_vec();
        let fit_buf = fit_buf.as_slice();

        let fit_image = Fit::from_bytes(fit_buf).unwrap();
        let fdt_image_from_fit = fit_image.get_image_by_node_path("/images/fdt-2").unwrap();

        let fdt2_image_data = include_bytes!("../test/data/platform-2.dtb").to_vec();
        let fdt2_image_buffer = fdt2_image_data.as_slice();

        assert_eq!(fdt2_image_buffer, fdt_image_from_fit);
    }
}
