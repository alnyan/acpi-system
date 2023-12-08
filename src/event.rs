use acpi::{
    address::{AccessSize, GenericAddress},
    AcpiHandler,
};
use alloc::{vec, vec::Vec};
use enum_map::Enum;

use crate::{
    hardware::{AcpiBitRegister, AcpiRegister},
    AcpiSleepState, AcpiSystem, AcpiSystemError, Handler,
};

pub const GPE_REGISTER_WIDTH: usize = 8;

#[allow(dead_code)]
struct GpeRegisterInfo {
    base_gpe_number: u16,
    enable_register: GenericAddress,
    status_register: GenericAddress,
}

#[allow(dead_code)]
struct GpeEventInfo {
    gpe_number: u16,
    register_index: usize,
}

#[allow(dead_code)]
pub(crate) struct GpeBlock {
    register_info: Vec<GpeRegisterInfo>,
    event_info: Vec<GpeEventInfo>,
    gpe_count: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Enum)]
pub(crate) enum EventHandlerId {
    Timer,
    GlobalLock,
    PowerButton,
    SleepButton,
    Rtc,
}

pub struct FixedEvent {
    pub(crate) name: &'static str,
    pub(crate) enable_register: AcpiBitRegister,
    pub(crate) status_register: AcpiBitRegister,
    pub(crate) handler_id: EventHandlerId,
}

// TODO event handlers cannot borrow AcpiSystem mutably, so some kind of "return token" has to be
//      used instead
#[derive(Clone, Copy, Debug, Default)]
pub enum EventAction {
    #[default]
    Nothing,
    EnterSleepState(AcpiSleepState),
}

impl FixedEvent {
    const LIST: &'static [&'static Self] = &[
        &Self::TIMER,
        &Self::GLOBAL_LOCK,
        &Self::POWER_BUTTON,
        &Self::SLEEP_BUTTON,
        &Self::RTC,
    ];

    pub const TIMER: Self = Self {
        name: "Timer",
        enable_register: AcpiBitRegister::new(AcpiRegister::Pm1Enable, 0),
        status_register: AcpiBitRegister::new(AcpiRegister::Pm1Status, 0),
        handler_id: EventHandlerId::Timer,
    };
    pub const GLOBAL_LOCK: Self = Self {
        name: "Global Lock",
        enable_register: AcpiBitRegister::new(AcpiRegister::Pm1Enable, 5),
        status_register: AcpiBitRegister::new(AcpiRegister::Pm1Status, 5),
        handler_id: EventHandlerId::GlobalLock,
    };
    pub const POWER_BUTTON: Self = Self {
        name: "Power Button",
        enable_register: AcpiBitRegister::new(AcpiRegister::Pm1Enable, 8),
        status_register: AcpiBitRegister::new(AcpiRegister::Pm1Status, 8),
        handler_id: EventHandlerId::PowerButton,
    };
    pub const SLEEP_BUTTON: Self = Self {
        name: "Sleep Button",
        enable_register: AcpiBitRegister::new(AcpiRegister::Pm1Enable, 9),
        status_register: AcpiBitRegister::new(AcpiRegister::Pm1Status, 9),
        handler_id: EventHandlerId::SleepButton,
    };
    pub const RTC: Self = Self {
        name: "RTC",
        enable_register: AcpiBitRegister::new(AcpiRegister::Pm1Enable, 10),
        status_register: AcpiBitRegister::new(AcpiRegister::Pm1Status, 10),
        handler_id: EventHandlerId::Rtc,
    };
}

impl<'a, H: Handler + AcpiHandler + 'a> AcpiSystem<'a, H> {
    // Event initialization
    pub(crate) fn initialize_events(&mut self) -> Result<(), AcpiSystemError> {
        self.initialize_fixed_events()?;
        self.initialize_gpes()?;

        self.install_sci_handler()?;

        Ok(())
    }

    fn disable_fixed_events(&mut self) -> Result<(), AcpiSystemError> {
        for fixed_event in FixedEvent::LIST {
            log::trace!("Disable fixed event: {:?}", fixed_event.name);
            fixed_event.enable_register.set(self, false)?;
        }
        Ok(())
    }

    fn initialize_fixed_events(&mut self) -> Result<(), AcpiSystemError> {
        self.disable_fixed_events()
    }

    fn install_sci_handler(&mut self) -> Result<(), AcpiSystemError> {
        let sci_interrupt = self.fadt.sci_interrupt as u32;
        H::install_interrupt_handler(sci_interrupt)
    }

    fn initialize_gpe_block(
        &mut self,
        block_address: GenericAddress,
        register_count: usize,
        block_base_number: u16,
        _interrupt_number: u32,
    ) -> Result<GpeBlock, AcpiSystemError> {
        log::info!("GPE block #{}", block_base_number);
        log::info!("Block address: {:#x?}", block_address);

        let mut register_info = vec![];
        let mut event_info = vec![];

        // GPEs are grouped in registers, each represented by one bit.
        // GPEx_STS register array is followed by GPEx_EN register array.
        let gpe_count = register_count * GPE_REGISTER_WIDTH;

        for i in 0..register_count {
            let base_gpe_number = block_base_number + i as u16 * GPE_REGISTER_WIDTH as u16;

            let status_register = GenericAddress {
                address: block_address.address + i as u64,
                address_space: block_address.address_space,
                bit_width: GPE_REGISTER_WIDTH as u8,
                bit_offset: 0,
                access_size: AccessSize::Undefined,
            };
            let enable_register = GenericAddress {
                address: block_address.address + i as u64 + register_count as u64,
                address_space: block_address.address_space,
                bit_width: GPE_REGISTER_WIDTH as u8,
                bit_offset: 0,
                access_size: AccessSize::Undefined,
            };

            // Initialize the GpeEventInfo
            for j in 0..GPE_REGISTER_WIDTH {
                let gpe_number = base_gpe_number + j as u16;

                event_info.push(GpeEventInfo {
                    gpe_number,
                    register_index: i,
                });
            }

            // Disable all GPEs within this register
            Self::write_address(enable_register, 0x00)?;

            // Clear any pending GPEs within this register
            Self::write_address(status_register, 0xFF)?;

            register_info.push(GpeRegisterInfo {
                base_gpe_number,
                status_register,
                enable_register,
            });
        }

        Ok(GpeBlock {
            register_info,
            event_info,
            gpe_count,
        })
    }

    fn initialize_gpes(&mut self) -> Result<(), AcpiSystemError> {
        // GPEx block contains a pair of GPEx_STS and GPEx_EN registers
        // Sizes of these registers equal to GPEx_LEN / 2
        //
        // GPE register width is 8 bits

        let _gpe_number_max = if let Some(gpe0) = self.fadt.gpe0_block()? {
            let reg_count = self.fadt.gpe0_block_length() as usize / 2;
            let gpe_number_max = (reg_count * GPE_REGISTER_WIDTH) - 1;

            // AcpiEvCreateGpeBlock
            let block =
                self.initialize_gpe_block(gpe0, reg_count, 0, self.fadt.sci_interrupt as u32)?;
            self.gpe0_block.replace(block);

            gpe_number_max
        } else {
            0
        };

        if let Some(_gpe1) = self.fadt.gpe1_block()? {
            todo!()
        }

        Ok(())
    }

    // Event handling
    pub(crate) fn handle_fixed_event_sci(&mut self) -> Result<(), AcpiSystemError> {
        let fixed_sts = self.read_register(AcpiRegister::Pm1Status)?;
        let fixed_en = self.read_register(AcpiRegister::Pm1Enable)?;

        for &event in FixedEvent::LIST {
            if event.enable_register.get_from_raw(fixed_en)
                && event.status_register.get_from_raw(fixed_sts)
            {
                log::trace!("Got event: {:?}", event.name);

                event.status_register.set(self, true).ok();

                // Clear the event by writing 1 into its status bit
                if let Some(handler) = &self.event_handlers[event.handler_id] {
                    self.handle_event_action(handler(self)).ok();
                }
            }
        }

        Ok(())
    }

    pub(crate) fn clear_fixed_events(&mut self) -> Result<(), AcpiSystemError> {
        log::trace!("Clear fixed events");
        let value = self.read_register(AcpiRegister::Pm1Status)?;
        let mut result = 0;

        for &event in FixedEvent::LIST {
            if event.status_register.get_from_raw(value) {
                log::trace!("Clear event: {:?}", event.name);
                result = event.status_register.set_raw(result, true);
            }
        }

        self.write_register(AcpiRegister::Pm1Status, value)
    }
}
