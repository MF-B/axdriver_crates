//! AHCI (Advanced Host Controller Interface) driver for SATA devices

extern crate alloc;
use crate::BlockDriverOps;
use axdriver_base::{BaseDriverOps, DevError, DevResult, DeviceType};

use ahci_driver::drv_ahci::{ahci_init, ahci_sata_read_common, ahci_sata_write_common};
use ahci_driver::libahci::{ahci_device, ahci_blk_dev};
use core::mem::MaybeUninit;

// ATA ID constants
const ATA_ID_SERNO_LEN: u32 = 20;
const ATA_ID_FW_REV_LEN: u32 = 8;
const ATA_ID_PROD_LEN: u32 = 40;

#[derive(Copy, Clone)]
#[repr(C, align(8))]
pub struct ahci_cmd_hdr {
    pub opts: u32,
    pub status: u32,
    pub tbl_addr_lo: u32,
    pub tbl_addr_hi: u32,
    pub reserved: [u32; 4],
}

#[derive(Copy, Clone)]
#[repr(C, align(8))]
pub struct ahci_sg {
    pub addr_lo: u32,
    pub addr_hi: u32,
    pub reserved: u32,
    pub flags_size: u32,
}

#[derive(Copy, Clone)]
#[repr(C, align(8))]
pub struct ahci_ioport {
    pub port_mmio: u64,
    pub cmd_slot: *mut ahci_cmd_hdr,
    pub cmd_slot_dma: u64,
    pub rx_fis: u64,
    pub rx_fis_dma: u64,
    pub cmd_tbl: u64,
    pub cmd_tbl_dma: u64,
    pub cmd_tbl_sg: *mut ahci_sg,
}

#[derive(Copy, Clone)]
#[repr(C, align(8))]
pub struct ahci_blk_dev {
    pub lba48: bool,
    pub _pad1: [u8; 7],              // 对齐到8字节边界
    pub lba: u64,
    pub blksz: u64,
    pub queue_depth: u32,
    pub _pad2: [u8; 4],              // 对齐到8字节边界
    pub product: [u8; (ATA_ID_PROD_LEN + 1) as usize],   // 41字节
    pub _pad3: [u8; 7],              // 填充到8字节对齐 (41 + 7 = 48, 48 % 8 = 0)
    pub serial: [u8; (ATA_ID_SERNO_LEN + 1) as usize],    // 21字节
    pub _pad4: [u8; 3],              // 填充到8字节对齐 (21 + 3 = 24, 24 % 8 = 0)
    pub revision: [u8; (ATA_ID_FW_REV_LEN + 1) as usize], // 9字节
    pub _pad5: [u8; 7],              // 填充到8字节对齐 (9 + 7 = 16, 16 % 8 = 0)
}

#[derive(Copy, Clone)]
#[repr(C, align(8))]
pub struct ahci_device {
    pub mmio_base: u64,

    pub flags: u32,

    pub cap: u32,
    pub cap2: u32,
    pub version: u32,
    pub port_map: u32,

    pub pio_mask: u32,
    pub udma_mask: u32,

    pub n_ports: u8, // num of ports
    pub port_map_linkup: u32,
    pub port: [ahci_ioport; 32],
    pub port_idx: u8, // the enabled port

    pub blk_dev: ahci_blk_dev,
}

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
            port: [ahci_ioport {
                port_mmio: 0,
                cmd_slot: core::ptr::null_mut(),
                cmd_slot_dma: 0,
                rx_fis: 0,
                rx_fis_dma: 0,
                cmd_tbl: 0,
                cmd_tbl_dma: 0,
                cmd_tbl_sg: core::ptr::null_mut(),
            }; 32],
            port_idx: 0, // the enabled port

            blk_dev: ahci_blk_dev {
                lba48: false,
                _pad1: [0; 7],              // 对齐到8字节边界
                lba: 0,
                blksz: 0,
                queue_depth: 0,
                _pad2: [0; 4],              // 对齐到8字节边界
                product: [0; (ATA_ID_PROD_LEN + 1) as usize],   // 41字节
                _pad3: [0; 7],              // 填充到8字节对齐 (41 + 7 = 48, 48 % 8 = 0)
                serial: [0; (ATA_ID_SERNO_LEN + 1) as usize],    // 21字节
                _pad4: [0; 3],              // 填充到8字节对齐 (21 + 3 = 24, 24 % 8 = 0)
                revision: [0; (ATA_ID_FW_REV_LEN + 1) as usize], // 9字节
                _pad5: [0; 7],              // 填充到8字节对齐 (9 + 7 = 16, 16 % 8 = 0)
            },
        };

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
