#![no_std]

extern crate alloc;

use acpi::{
    fadt::{Fadt, FadtRegisters},
    AcpiHandler,
    AcpiTables,
    PhysicalMapping,
};
use alloc::{boxed::Box, vec};
use aml::{AmlContext, AmlError, AmlName, AmlValue};
use event::GpeBlock;

mod data;
mod error;
mod event;
mod hardware;
mod sleep;

pub use error::AcpiSystemError;
pub use event::FixedEvent;
pub use sleep::AcpiSleepState;

const PATH_PIC: &str = "\\_PIC";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AcpiInterruptMethod {
    Pic = 0,
    Apic = 1,
    SApic = 2,
}

// TODO maybe merge this with aml handler?
pub trait Handler: Clone {
    fn install_interrupt_handler(irq: u32) -> Result<(), AcpiSystemError>;

    unsafe fn map_slice(address: u64, length: u64) -> &'static [u8];

    fn io_read_u8(port: u16) -> u8;
    fn io_read_u16(port: u16) -> u16;
    fn io_read_u32(port: u16) -> u32;

    fn io_write_u8(port: u16, value: u8);
    fn io_write_u16(port: u16, value: u16);
    fn io_write_u32(port: u16, value: u32);

    fn mem_read_u8(address: u64) -> u8;
    fn mem_read_u16(address: u64) -> u16;
    fn mem_read_u32(address: u64) -> u32;
    fn mem_read_u64(address: u64) -> u64;
}

pub struct AcpiSystem<'a, H: Handler + AcpiHandler + 'a> {
    tables: &'a AcpiTables<H>,
    aml_context: AmlContext,

    // FADT and its PMx registers
    fadt: PhysicalMapping<H, Fadt>,
    fadt_registers: FadtRegisters,

    // Event handling
    gpe0_block: Option<GpeBlock>,
    #[allow(dead_code)]
    gpe1_block: Option<GpeBlock>,
}

impl<'a, H: Handler + AcpiHandler + 'a> AcpiSystem<'a, H> {
    pub fn new(tables: &'a AcpiTables<H>, aml_handler: Box<dyn aml::Handler>) -> Result<Self, AcpiSystemError> {
        let fadt = tables.find_table::<Fadt>()?;
        let fadt_registers = fadt.pm_registers()?;

        let aml_context = AmlContext::new(aml_handler, aml::DebugVerbosity::None);

        Ok(Self { tables, aml_context, fadt, fadt_registers, gpe0_block: None, gpe1_block: None })
    }

    pub fn initialize(&mut self, interrupt_method: AcpiInterruptMethod) -> Result<(), AcpiSystemError> {
        // Enable hardware part of ACPI
        self.enable_acpi()?;

        // TODO load SSDTs, `aml` currently can't handle them on my T430
        // TODO use find_table() instead (which won't work for SSDTs, because there may be multiple
        //      of them)
        if let Ok(dsdt) = self.tables.dsdt() {
            let dsdt = unsafe { H::map_slice(dsdt.address as _, dsdt.length as _) };

            self.aml_context
                .parse_table(dsdt)
                .map_err(|e| {
                    log::error!("Could not initialize DSDT: {:?}", e);
                    e
                })
                .unwrap();
        }

        self.initialize_events()?;

        self.aml_context.initialize_objects()?;

        self.configure_aml_interrupt_method(interrupt_method)?;

        Ok(())
    }

    pub fn enable_acpi(&mut self) -> Result<(), AcpiSystemError> {
        let state = self.is_acpi_enabled()?;
        log::trace!("Current ACPI status: {:?}", state);

        if !state {
            self.set_acpi_mode(true)?;
        }

        Ok(())
    }

    pub fn enable_fixed_event(&mut self, event: &FixedEvent) -> Result<(), AcpiSystemError> {
        log::info!("Enable ACPI event: {}", event.name);
        event.enable_register.set(self, true)
    }

    pub fn handle_sci(&mut self) {
        if let Err(err) = self.handle_fixed_event_sci() {
            log::warn!("{:?}", err);
        }
        // TODO handle GPEs
    }

    pub unsafe fn enter_sleep_state(&mut self, state: AcpiSleepState) -> Result<(), AcpiSystemError> {
        log::info!("Entering sleep state: {:?}", state);
        let (sleep_type_a, sleep_type_b) = self.prepare_sleep_state(state)?;
        self.dispatch_sleep_command(sleep_type_a, sleep_type_b)
    }

    fn configure_aml_interrupt_method(
        &mut self,
        interrupt_method: AcpiInterruptMethod,
    ) -> Result<(), AcpiSystemError> {
        let value = interrupt_method as u64;
        let path = AmlName::from_str(PATH_PIC).unwrap();
        let args = aml::value::Args::from_list(vec![AmlValue::Integer(value)]).unwrap();

        match self.aml_context.invoke_method(&path, args) {
            Ok(_) | Err(AmlError::ValueDoesNotExist(_)) => Ok(()),
            Err(err) => Err(AcpiSystemError::AmlError(err)),
        }
    }
}
