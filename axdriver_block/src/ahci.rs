//! AHCI (Advanced Host Controller Interface) driver for SATA devices

extern crate alloc;
use crate::BlockDriverOps;
use axdriver_base::{BaseDriverOps, DevError, DevResult, DeviceType};

use ahci_driver::drv_ahci::{ahci_init, ahci_sata_read_common, ahci_sata_write_common};
use ahci_driver::libahci::ahci_device;
use core::mem::MaybeUninit;

/// AHCI driver implementation
pub struct AhciDriver {
    /// AHCI device structure containing all the necessary hardware information
    device: ahci_device,
}

impl AhciDriver {
    /// Initialize the AHCI driver, returns `Ok` if successful.
    pub fn try_new() -> DevResult<AhciDriver> {
        // Create an uninitialized AHCI device structure
        let mut device = MaybeUninit::<ahci_device>::uninit();

        // Initialize the device structure to zero
        unsafe {
            core::ptr::write_bytes(device.as_mut_ptr(), 0, 1);
        }

        let mut device = unsafe { device.assume_init() };

        // Call the C-style initialization function
        let result = unsafe { ahci_init(&mut device) };

        if result == 0 {
            log::info!("AHCI: successfully initialized");
            Ok(AhciDriver { device })
        } else {
            log::warn!("AHCI: init failed with error code {}", result);
            Err(DevError::Io)
        }
    }

    /// Get a reference to the underlying AHCI device
    pub fn device(&self) -> &ahci_device {
        &self.device
    }
}

impl BaseDriverOps for AhciDriver {
    fn device_type(&self) -> DeviceType {
        DeviceType::Block
    }

    fn device_name(&self) -> &str {
        "ahci"
    }
}

impl BlockDriverOps for AhciDriver {
    fn read_block(&mut self, block_id: u64, buf: &mut [u8]) -> DevResult {
        let block_size = self.block_size();
        let block_count = (buf.len() + block_size - 1) / block_size;

        if buf.len() % block_size != 0 {
            log::warn!(
                "Buffer size {} is not aligned to block size {}",
                buf.len(),
                block_size
            );
        }

        // Call the underlying AHCI read function
        let result = unsafe {
            ahci_sata_read_common(&self.device, block_id, block_count as u32, buf.as_mut_ptr())
        };

        if result == block_count as u64 {
            Ok(())
        } else {
            log::error!(
                "AHCI read failed: expected {} blocks, got {}",
                block_count,
                result
            );
            Err(DevError::Io)
        }
    }

    fn write_block(&mut self, block_id: u64, buf: &[u8]) -> DevResult {
        let block_size = self.block_size();
        let block_count = (buf.len() + block_size - 1) / block_size;

        if buf.len() % block_size != 0 {
            log::warn!(
                "Buffer size {} is not aligned to block size {}",
                buf.len(),
                block_size
            );
        }

        // Call the underlying AHCI write function
        let result = unsafe {
            ahci_sata_write_common(
                &self.device,
                block_id,
                block_count as u32,
                buf.as_ptr() as *mut u8, // Cast away const for C interface
            )
        };

        if result == block_count as u64 {
            Ok(())
        } else {
            log::error!(
                "AHCI write failed: expected {} blocks, got {}",
                block_count,
                result
            );
            Err(DevError::Io)
        }
    }

    fn flush(&mut self) -> DevResult {
        // The AHCI write function already handles cache flushing based on device flags
        // No additional flush operation is needed as it's handled internally
        Ok(())
    }

    #[inline]
    fn num_blocks(&self) -> u64 {
        // Return the LBA (Logical Block Address) count from the device
        self.device.blk_dev.lba
    }

    #[inline]
    fn block_size(&self) -> usize {
        // Return the block size from the device, typically 512 bytes for SATA drives
        self.device.blk_dev.blksz as usize
    }
}
