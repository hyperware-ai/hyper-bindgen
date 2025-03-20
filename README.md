# hyper-bindgen

A tool to generate WIT (WebAssembly Interface Type) files from Rust hyperprocess implementations.

## Overview

hyper-bindgen analyzes Rust code that uses the hyperprocess attribute and automatically generates corresponding WIT interface definitions. This simplifies the development workflow when working with WebAssembly components that use the WIT format.

## Features

- Automatically scans Rust projects for hyperprocess implementations
- Extracts interface definitions and method signatures
- Converts Rust types to WIT types
- Generates properly formatted WIT files
- Validates naming conventions according to WIT standards
- Supports kebab-case conversion for interface names

## Installation

```bash
cargo install --path .  
```

## Usage

```bash
# Run in the root directory of your Rust project
hyper-bindgen

# The tool will:
# 1. Find all Rust files with hyperprocess implementations
# 2. Generate corresponding WIT files in the api/ directory
```

## Example

For a Rust implementation like:

```rust
#[hyperprocess(wit_world = "my-component")]
impl MyService {
    fn get_data(&self, id: String) -> Result<Vec<DataItem>, Error> {
        // implementation
    }
}
```

hyper-bindgen will generate a WIT file like:

```wit
interface my-service {
  record data-item {
    // fields
  }
  
  variant error {
    // error variants
  }
  
  get-data: func(id: string) -> result<list<data-item>, error>
}
```

## Requirements

- Rust 2021 edition or newer
- The following dependencies:
  - anyhow 1.0
  - syn 2.0 (with features: full, parsing, extra-traits)
  - walkdir 2.3
  - toml 0.7

## License

MIT

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request. 