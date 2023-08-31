use acpi::AcpiHandler;
use alloc::vec;
use aml::{AmlError, AmlName, AmlValue};

use crate::{
    hardware::{AcpiBitRangeRegister, AcpiBitRegister, AcpiRegister},
    AcpiSystem, AcpiSystemError, Handler,
};

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum AcpiSleepState {
    S0 = 0,
    S1 = 1,
    S2 = 2,
    S3 = 3,
    S4 = 4,
    S5 = 5,
}

const SLEEP_STATE_NAMES: &[&str] = &["\\_S0_", "\\_S1_", "\\_S2_", "\\_S3_", "\\_S4_", "\\_S5_"];
const PATH_PREPARE_TO_SLEEP: &str = "\\_PTS";
const PATH_SYSTEM_STATUS: &str = "\\_SI._SST";

impl<'a, H: Handler + AcpiHandler + 'a> AcpiSystem<'a, H> {
    fn sleep_type_data(&self, state: AcpiSleepState) -> Result<(u8, u8), AcpiSystemError> {
        // Evaluate the \_Sx namespace object containing the register values
        let name = SLEEP_STATE_NAMES[state as usize];
        let path = AmlName::from_str(name).unwrap();
        let info = self
            .aml_context
            .namespace
            .get_by_path(&path)
            .map_err(|err| match err {
                AmlError::ValueDoesNotExist(_) => AcpiSystemError::MissingSleepMethod(name),
                e => e.into(),
            })?;

        let AmlValue::Package(elements) = info else {
            return Err(AcpiSystemError::InvalidSleepMethod(name));
        };

        match elements.len() {
            0 => todo!(),
            1 => todo!(),
            _ => {
                let val_a = elements[0]
                    .as_integer(&self.aml_context)
                    .map_err(|_| AcpiSystemError::InvalidSleepMethod(name))?
                    as u8;
                let val_b = elements[1]
                    .as_integer(&self.aml_context)
                    .map_err(|_| AcpiSystemError::InvalidSleepMethod(name))?
                    as u8;

                Ok((val_a, val_b))
            }
        }
    }

    pub(crate) unsafe fn prepare_sleep_state(
        &mut self,
        state: AcpiSleepState,
    ) -> Result<(u8, u8), AcpiSystemError> {
        let sleep_types = self.sleep_type_data(state)?;

        // Invoke \_PTS (Prepare to sleep)
        let args = aml::value::Args::from_list(vec![AmlValue::Integer(state as _)]).unwrap();
        let path = AmlName::from_str(PATH_PREPARE_TO_SLEEP).unwrap();

        if let Err(err) = self.aml_context.invoke_method(&path, args) {
            if !matches!(err, AmlError::ValueDoesNotExist(_)) {
                return Err(AcpiSystemError::AmlError(err));
            }

            log::warn!("{}: {:?}", PATH_PREPARE_TO_SLEEP, err);
        }

        // Setup the argument to the _SST method (System STatus)
        let sst_value = match state {
            AcpiSleepState::S0 => todo!(),
            AcpiSleepState::S1 | AcpiSleepState::S2 | AcpiSleepState::S3 => todo!(),
            AcpiSleepState::S4 => todo!(),
            AcpiSleepState::S5 => 0, /* ACPI_SST_INDICATOR_OFF */
        };

        let path = AmlName::from_str(PATH_SYSTEM_STATUS).unwrap();
        let args = aml::value::Args::from_list(vec![AmlValue::Integer(sst_value as _)]).unwrap();

        if let Err(err) = self.aml_context.invoke_method(&path, args) {
            if !matches!(err, AmlError::ValueDoesNotExist(_)) {
                return Err(AcpiSystemError::AmlError(err));
            }

            log::warn!("{}: {:?}", PATH_SYSTEM_STATUS, err);
        }

        Ok(sleep_types)
    }

    unsafe fn acpi_hw_legacy_sleep(
        &mut self,
        sleep_type_a: u8,
        sleep_type_b: u8,
    ) -> Result<(), AcpiSystemError> {
        let sleep_type_reg = &AcpiBitRangeRegister::SLEEP_TYPE;
        let sleep_enable_reg = &AcpiBitRegister::SLEEP_ENABLE;

        self.clear_fixed_events()?;

        // Clear wake status
        AcpiBitRegister::WAKE_STATUS.set(self, true)?;
        // TODO disable all GPEs
        // TODO enable all wakeup GPEs

        // Get current pm1a control value
        let mut pm1_control = self.read_register(AcpiRegister::Pm1Control)?;

        // Clear SLP_TYP field
        pm1_control = sleep_enable_reg.set_raw(pm1_control, false);

        let pm1a_control = sleep_type_reg.set_raw(pm1_control, sleep_type_a as u32);
        let pm1b_control = sleep_type_reg.set_raw(pm1_control, sleep_type_b as u32);

        // Write Pm1Control back with modified SLP_TYP and clear SLP_EN
        self.write_pm1_control(pm1a_control, pm1b_control)?;

        unsafe {
            H::flush_cpu_cache();
        }

        // Now write Pm1Control again, this time with SLP_EN set
        self.write_pm1_control(
            sleep_enable_reg.set_raw(pm1a_control, true),
            sleep_enable_reg.set_raw(pm1b_control, true),
        )?;

        H::halt()
    }

    pub(crate) unsafe fn dispatch_sleep_command(
        &mut self,
        sleep_type_a: u8,
        sleep_type_b: u8,
    ) -> Result<(), AcpiSystemError> {
        if sleep_type_a > 7 || sleep_type_b > 7 {
            return Err(AcpiSystemError::InvalidSleepValues(
                sleep_type_a,
                sleep_type_b,
            ));
        }

        self.acpi_hw_legacy_sleep(sleep_type_a, sleep_type_b)?;

        Ok(())
    }
}
