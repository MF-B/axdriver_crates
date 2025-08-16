//! AHCI (Advanced Host Controller Interface) driver for SATA devices

extern crate alloc;
use crate::BlockDriverOps;
use axdriver_base::{BaseDriverOps, DevError, DevResult, DeviceType};

use ahci_driver::drv_ahci::{ahci_init, ahci_sata_read_common, ahci_sata_write_common};
use ahci_driver::libahci::{ahci_device, ahci_blk_dev};
use core::mem::MaybeUninit;

/// AHCI driver implementation
pub struct AhciDriver {
    /// AHCI device structure containing all the necessary hardware information
    device: ahci_device,
}

impl AhciDriver {
    /// Initialize the AHCI driver, returns `Ok` if successful.
    pub fn try_new() -> DevResult<AhciDriver> {
        log::info!("AHCI: initializing");
        // Create an uninitialized AHCI device structure
        let mut device = ahci_device {
            mmio_base: 0,
            // Initialize other fields as needed
            flags: 0,
            cap: 0,
            cap2: 0,
            version: 0,
            port_map: 0,
            pio_mask: 0,
            udma_mask: 0,
            n_ports: 0,
            port_map_linkup: 0,
            port: [0; 32],
            port_idx: 0, // the enabled port

            blk_dev: ahci_device_dev {
                lba48: false,
                _pad1: [0; 7],              // 对齐到8字节边界
                lba: 0,
                blksz: 0,
                queue_depth: 0,
                _pad2: [0; 4],              // 对齐到8字节边界
                product: [0; 41],   // 41字节
                _pad3: [0; 7],              // 填充到8字节对齐 (41 + 7 = 48, 48 % 8 = 0)
                serial: [0; 21],    // 21字节
                _pad4: [0; 3],              // 填充到8字节对齐 (21 + 3 = 24, 24 % 8 = 0)
                revision: [0; 9], // 9字节
                _pad5: [0; 7],              // 填充到8字节对齐 (9 + 7 = 16, 16 % 8 = 0)
            },
        };  

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
