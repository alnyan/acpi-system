`acpi-system`
=============

The library provides a way to manage ACPI hardware 
for operating systems written in Rust. It is designed
to be simple to integrate and is based on 
[`acpi`](https://github.com/rust-osdev/acpi) crates.

The crate has not yet been published to crates.io and is 
in its early development stage.

See the "Issues" section of this repo for any major 
problems before using this crate.

Usage example
-------------

```rust
#[derive(Clone)]
struct MyHandler;
struct MySciHandler;

impl aml::Handler for MyHandler {
	// ...
}

impl acpi::AcpiHandler for MyHandler {
	// ...
}

impl acpi_system::Handler for MyHandler {
	// ...
}

// ...

fn my_acpi_init() {
	let tables = // ... obtain ACPI tables, see acpi crate docs
	let mut system = AcpiSystem::new(Box::new(MyHandler));

	system.initialize(AcpiInterruptMethod::Apic).unwrap();

	// At this point, AcpiSystem is usable and we can do things like
	// powering down the system or binding events:
	system.enable_fixed_event(&FixedEvent::POWER_BUTTON).unwrap();
}
```

Contributing
------------

As the project has only been started recently, any contributions are welcome:

* Test this crate with your hardware (I don't promise it won't burst into flames.
* Write code for missing features
* Open issues in this repo

ACPICA and the ACPI specification were used as main sources of 
information for this project, so if you decide to contribute,
you may find them very useful.