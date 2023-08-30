use acpi::AcpiError;
use aml::AmlError;

#[derive(Debug)]
pub enum AcpiSystemError {
    AcpiError(AcpiError),
    AmlError(AmlError),
}

impl From<AcpiError> for AcpiSystemError {
    fn from(value: AcpiError) -> Self {
        Self::AcpiError(value)
    }
}

impl From<AmlError> for AcpiSystemError {
    fn from(value: AmlError) -> Self {
        Self::AmlError(value)
    }
}
