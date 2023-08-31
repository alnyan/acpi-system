use core::{ops::Range, time::Duration};

use acpi::{
    address::{AccessSize, AddressSpace, GenericAddress},
    AcpiHandler,
};
use bit_field::BitField;

use crate::{AcpiSystem, AcpiSystemError, Handler};

pub const PM1_STATUS_PRESERVED_BITS: u32 = 1 << 11;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AcpiRegister {
    Pm1Status,
    Pm1Control,
    Pm1Enable,
}

pub(crate) struct AcpiBitRegister {
    parent: AcpiRegister,
    position: usize,
}

pub(crate) struct AcpiBitRangeRegister {
    range: Range<usize>,
}

impl AcpiBitRegister {
    pub(crate) const SCI_ENABLE: Self = Self {
        parent: AcpiRegister::Pm1Control,
        position: 0,
    };
    pub(crate) const SLEEP_ENABLE: Self = Self {
        parent: AcpiRegister::Pm1Control,
        position: 13,
    };
    pub(crate) const WAKE_STATUS: Self = Self {
        parent: AcpiRegister::Pm1Status,
        position: 15,
    };

    pub(crate) const fn new(parent: AcpiRegister, position: usize) -> Self {
        Self { parent, position }
    }
}

impl AcpiBitRangeRegister {
    pub(crate) const SLEEP_TYPE: Self = Self { range: 10..13 };
}

impl AcpiBitRegister {
    pub fn set<'a, H: Handler + AcpiHandler + 'a>(
        &self,
        context: &mut AcpiSystem<'a, H>,
        value: bool,
    ) -> Result<(), AcpiSystemError> {
        let mut reg_value = context.read_register(self.parent)?;
        reg_value.set_bit(self.position, value);
        context.write_register(self.parent, reg_value)
    }

    pub fn get<'a, H: Handler + AcpiHandler + 'a>(
        &self,
        context: &AcpiSystem<'a, H>,
    ) -> Result<bool, AcpiSystemError> {
        let reg_value = context.read_register(self.parent)?;
        Ok(reg_value.get_bit(self.position))
    }

    #[inline]
    pub fn get_from_raw(&self, value: u32) -> bool {
        value.get_bit(self.position)
    }

    #[inline]
    pub fn set_raw(&self, mut raw: u32, value: bool) -> u32 {
        raw.set_bit(self.position, value);
        raw
    }
}

impl AcpiBitRangeRegister {
    #[inline]
    pub fn set_raw(&self, mut raw: u32, value: u32) -> u32 {
        raw.set_bits(self.range.clone(), value);
        raw
    }
}

fn access_bit_width(register: &GenericAddress, address: u64, mut maximum_width: u8) -> u8 {
    let access_bit_width = if register.bit_offset == 0
        && register.bit_width != 0
        && register.bit_width.is_power_of_two()
        && register.bit_width % 8 == 0
    {
        register.bit_width
    } else if register.access_size != AccessSize::Undefined {
        match register.access_size {
            AccessSize::ByteAccess => 8,
            AccessSize::WordAccess => 16,
            AccessSize::DWordAccess => 32,
            AccessSize::QWordAccess => 64,
            _ => unimplemented!(),
        }
    } else {
        let mut width = (register.bit_offset + register.bit_width).next_power_of_two();

        if width < 8 {
            width = 8;
        } else {
            while address % width as u64 != 0 {
                width >>= 1;
            }
        }

        width
    };

    if register.address_space == AddressSpace::SystemIo {
        maximum_width = 32;
    }

    core::cmp::min(access_bit_width, maximum_width)
}

impl<'a, H: Handler + AcpiHandler + 'a> AcpiSystem<'a, H> {
    pub(crate) fn write_register(
        &mut self,
        register: AcpiRegister,
        value: u32,
    ) -> Result<(), AcpiSystemError> {
        match register {
            AcpiRegister::Pm1Status => {
                let value = value & !PM1_STATUS_PRESERVED_BITS;

                let pm1a = self.pm1_registers.x_pm1a_status;
                let pm1b = self.pm1_registers.x_pm1b_status;

                Self::write_register_pair(pm1a, pm1b, value)
            }
            AcpiRegister::Pm1Enable => {
                let pm1a = self.pm1_registers.x_pm1a_enable;
                let pm1b = self.pm1_registers.x_pm1b_enable;

                Self::write_register_pair(pm1a, pm1b, value)
            }
            AcpiRegister::Pm1Control => {
                // Pm1Control registers are hanled differently
                unimplemented!()
            }
        }
    }

    pub(crate) fn read_register(&self, register: AcpiRegister) -> Result<u32, AcpiSystemError> {
        match register {
            AcpiRegister::Pm1Status => {
                let pm1a = self.pm1_registers.x_pm1a_status;
                let pm1b = self.pm1_registers.x_pm1b_status;

                Self::read_register_pair(pm1a, pm1b)
            }
            AcpiRegister::Pm1Enable => {
                let pm1a = self.pm1_registers.x_pm1a_enable;
                let pm1b = self.pm1_registers.x_pm1b_enable;

                Self::read_register_pair(pm1a, pm1b)
            }
            AcpiRegister::Pm1Control => {
                let pm1a = self.fadt.pm1a_control_block()?;
                let pm1b = self.fadt.pm1b_control_block()?;

                Self::read_register_pair(pm1a, pm1b)
            }
        }
    }

    // A different function is needed for Pm1Control because we don't just write two copies of the
    // same value into this pair. Each register receives its own value instead.
    pub(crate) fn write_pm1_control(
        &mut self,
        reg_a_value: u32,
        reg_b_value: u32,
    ) -> Result<(), AcpiSystemError> {
        let pm1a = self.fadt.pm1a_control_block()?;
        let pm1b = self.fadt.pm1b_control_block()?;

        Self::write_address(pm1a, reg_a_value as u64)?;
        if let Some(pm1b) = pm1b {
            Self::write_address(pm1b, reg_b_value as u64)?;
        }

        Ok(())
    }

    fn write_register_pair(
        reg_a: GenericAddress,
        reg_b: Option<GenericAddress>,
        value: u32,
    ) -> Result<(), AcpiSystemError> {
        Self::write_address(reg_a, value as u64)?;
        if let Some(reg_b) = reg_b {
            Self::write_address(reg_b, value as u64)?;
        }
        Ok(())
    }

    fn read_register_pair(
        reg_a: GenericAddress,
        reg_b: Option<GenericAddress>,
    ) -> Result<u32, AcpiSystemError> {
        let value_a = Self::read_address(reg_a)? as u32;
        let value_b = if let Some(reg_b) = reg_b {
            Self::read_address(reg_b)? as u32
        } else {
            0
        };
        Ok(value_a | value_b)
    }

    fn read_address_space(
        space: AddressSpace,
        address: u64,
        width: usize,
    ) -> Result<u64, AcpiSystemError> {
        match space {
            AddressSpace::SystemMemory => match width {
                8 => Ok(H::mem_read_u8(address) as _),
                16 => Ok(H::mem_read_u16(address) as _),
                32 => Ok(H::mem_read_u32(address) as _),
                64 => Ok(H::mem_read_u64(address)),
                _ => unimplemented!(),
            },
            AddressSpace::SystemIo => {
                let address = address.try_into().unwrap();

                match width {
                    8 => Ok(H::io_read_u8(address) as _),
                    16 => Ok(H::io_read_u32(address) as _),
                    32 => Ok(H::io_read_u16(address) as _),
                    _ => unimplemented!(),
                }
            }
            _ => unimplemented!(),
        }
    }

    fn write_address_space(
        space: AddressSpace,
        address: u64,
        width: usize,
        value: u64,
    ) -> Result<(), AcpiSystemError> {
        match space {
            AddressSpace::SystemMemory => {
                todo!()
            }
            AddressSpace::SystemIo => {
                let address = address.try_into().unwrap();

                match width {
                    8 => H::io_write_u8(address, value as u8),
                    16 => H::io_write_u16(address, value as u16),
                    32 => H::io_write_u32(address, value as u32),
                    _ => unimplemented!(),
                };

                Ok(())
            }
            _ => unimplemented!(),
        }
    }

    // TODO I just copied this from ACPICA, needs a check and rewrite, because I don't really like
    //      their code
    pub(crate) fn read_address(reg: GenericAddress) -> Result<u64, AcpiSystemError> {
        // TODO ValidateRegister
        let mut value = 0;
        let address = reg.address;
        let access_width = access_bit_width(&reg, address, 64) as usize;
        let mut bit_width = (reg.bit_width + reg.bit_offset) as usize;
        let mut bit_offset = reg.bit_offset as usize;
        let mut index = 0;

        while bit_width != 0 {
            let data = if bit_offset >= access_width {
                bit_offset -= access_width;
                0
            } else {
                let access_address = address + (index * access_width / 8) as u64;
                Self::read_address_space(reg.address_space, access_address, access_width)?
            };

            let bit_position = index * access_width;
            let bits = data.get_bits(0..access_width);
            value.set_bits(bit_position..bit_position + access_width, bits);

            if bit_width > access_width {
                bit_width -= access_width;
            } else {
                break;
            }
            index += 1;
        }

        Ok(value)
    }

    pub(crate) fn write_address(reg: GenericAddress, value: u64) -> Result<(), AcpiSystemError> {
        let address = reg.address;
        let access_width = access_bit_width(&reg, address, 64) as usize;
        let mut bit_width = (reg.bit_width + reg.bit_offset) as usize;
        let mut bit_offset = reg.bit_offset as usize;
        let mut index = 0;

        while bit_width != 0 {
            let bit_position = index * access_width;
            let bits = value.get_bits(bit_position..bit_position + access_width);

            if bit_offset >= access_width {
                bit_offset -= access_width;
            } else {
                let access_address = address + (index * access_width / 8) as u64;
                Self::write_address_space(reg.address_space, access_address, access_width, bits)?;
            }

            if bit_width > access_width {
                bit_width -= access_width;
            } else {
                break;
            }
            index += 1;
        }

        Ok(())
    }

    pub(crate) fn set_acpi_mode(&mut self, acpi: bool) -> Result<(), AcpiSystemError> {
        const TIMEOUT: Duration = Duration::from_secs(1);

        if self.fadt.acpi_enable == 0 && self.fadt.acpi_disable == 0 {
            log::error!("No ACPI mode transition is supported in this system");
            return Err(AcpiSystemError::ModeTransitionNotSupported);
        }

        if acpi {
            Self::write_address_space(
                AddressSpace::SystemIo,
                self.fadt.smi_cmd_port as u64,
                8,
                self.fadt.acpi_enable as u64,
            )?;
        } else {
            todo!("Switch to non-ACPI is not supported yet")
        }

        let mut attempts = 3000;
        while attempts != 0 {
            let acpi_enabled = self.is_acpi_enabled().unwrap_or(false);

            if acpi_enabled {
                return Ok(());
            }

            H::stall(TIMEOUT);

            attempts -= 1;
        }

        Err(AcpiSystemError::EnableTimeout)
    }

    pub(crate) fn is_acpi_enabled(&mut self) -> Result<bool, AcpiSystemError> {
        if self.fadt.smi_cmd_port == 0 {
            return Ok(true);
        }

        let state = AcpiBitRegister::SCI_ENABLE.get(self).unwrap_or(false);

        Ok(state)
    }
}
